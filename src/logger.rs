use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::America::New_York;
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension, ToSql};
use std::{fs::OpenOptions, io::{self, Write}, thread, time::Duration as StdDuration};

// ------------------ PARAMETERS ------------------
pub const MAX_ATTEMPTS: usize = 5;          // Max failed attempts before lockout
const LOCKOUT_SECONDS_BASE: i64 = 30;   // Initial lockout (30s)
const MAX_LOCKOUT_SECONDS: i64 = 300;   // Max lockout cap (5 minutes)

// Current timestamp in Eastern Time (EST/EDT)
pub fn now_est() -> DateTime<chrono_tz::Tz> {
    New_York.from_utc_datetime(&Utc::now().naive_utc())
}

// Small random delay to prevent timing attacks
pub fn fake_verification_delay() {
    let delay_ms: u64 = rand::thread_rng().gen_range(100..=250);
    thread::sleep(StdDuration::from_millis(delay_ms));
}


// ------------------ LOGGING ------------------
// Log event to both DB and file
pub fn log_event(conn: &Connection, actor_username: &str, target_username: Option<&str>, event_type: &str, description: Option<&str>) -> Result<()> {
    let timestamp = now_est().to_rfc3339();

    conn.execute(
        "INSERT INTO security_log (actor_username, target_username, event_type, description, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![actor_username, target_username.unwrap_or(actor_username), event_type, description.unwrap_or(""), timestamp],
    )?;

    // Log to file
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("security.log")
        .context("Failed to open security.log")?;
    writeln!(
        file,
        "{} | actor={} | target={} | event={} | desc={}",
        timestamp, actor_username, target_username.unwrap_or("-"), event_type, description.unwrap_or("-")
    )?;
    Ok(())
}

// ------------------ LOCKOUT SYSTEM ------------------

// Check if user is currently locked out
pub fn check_lockout(conn: &Connection, username: &str) -> Result<bool> {
    if let Some(locked_until_str) = conn
        .query_row(
            "SELECT locked_until FROM lockouts WHERE username = ?1 COLLATE NOCASE",
            params![username],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        let now = now_est();
        let locked_until =
            DateTime::parse_from_rfc3339(&locked_until_str)?.with_timezone(&New_York);

        if now < locked_until {
            let remaining = (locked_until - now).num_seconds();
            println!(
                "Account '{}' is locked for another {} seconds (until {}).",
                username,
                remaining,
                locked_until.format("%Y-%m-%d %H:%M:%S %Z")
            );
             return Ok(true);
        } else {
            conn.execute("DELETE FROM lockouts WHERE username = ?1", params![username])?;
        }
    }
    Ok(false)
}

// Record success/failure and apply lockouts automatically
pub fn record_login_attempt(conn: &Connection, actor_username: &str, success: bool) -> Result<()> {
    if success {
        log_event(conn, actor_username, Some(actor_username), "SUCCESS_LOGIN", None)?;

         // Update last_login_time and updated_at
        conn.execute(
            "UPDATE users 
             SET last_login_time = datetime('now'), updated_at = datetime('now')
             WHERE username = ?1 COLLATE NOCASE",
            params![actor_username],
        )?;

        // Clear any lockout state for this user
        conn.execute("DELETE FROM lockouts WHERE username = ?1", params![actor_username])?;
        return Ok(());
    }

    // Failed attempt
    log_event(conn, actor_username, Some(actor_username), "FAILURE_LOGIN", None)?;

   let recent_failures: i64 = conn.query_row(
        r#"
        SELECT COUNT(*) FROM security_log
        WHERE actor_username = ?1
          AND event_type = 'FAILURE_LOGIN'
          AND timestamp > IFNULL((
                SELECT MAX(timestamp)
                FROM security_log
                WHERE actor_username = ?1 AND event_type = 'SUCCESS_LOGIN'
          ), '1970-01-01T00:00:00Z')
          AND timestamp > datetime('now', '-5 minutes')
        "#,
        params![actor_username],
        |r| r.get(0),
    )?;


    if recent_failures >= MAX_ATTEMPTS as i64 {
        // Get previous lockout count (if exists)
        let prev_count: Option<i64> = conn
            .query_row(
                "SELECT lock_count FROM lockouts WHERE username = ?1",
                params![actor_username],
                |r| r.get(0),
            )
            .optional()?;

        let next_count = prev_count.map_or(1, |c| (c + 1).min(10));
        let lockout_secs =
            (LOCKOUT_SECONDS_BASE * (2_i64.pow(next_count as u32 - 1))).min(MAX_LOCKOUT_SECONDS);
        let locked_until = (now_est() + Duration::seconds(lockout_secs)).to_rfc3339();

        // Store new lockout
        conn.execute(
            "INSERT OR REPLACE INTO lockouts (username, locked_until, lock_count)
             VALUES (?1, ?2, ?3)",
            params![actor_username, locked_until, next_count],
        )?;

        log_event(conn, actor_username, Some(actor_username), "LOCKOUT", 
        Some("Account locked due to repeated failed attempts."))?;
        println!(
            "'{}' locked for {} seconds (until {}).",
            actor_username, lockout_secs, locked_until
        );
    }

    Ok(())
}


// ======================= SESSION LOCKOUT ====================

pub fn increment_session_fail(conn: &Connection, username: Option<&str>) -> Result<()> {
    if let Some(u) = username {
        // Logged-in or specific user
        conn.execute(
            "UPDATE session_state
             SET failed_attempts = failed_attempts + 1
             WHERE username = ?1 COLLATE NOCASE",
            params![u],
        )?;
    } else {
        // Anonymous / pre-login session
        conn.execute(
            "UPDATE session_state
             SET failed_attempts = failed_attempts + 1
             WHERE username IS NULL",
            [],
        )?;
    }

    Ok(())
}


pub fn session_lockout_check(conn: &Connection, username: Option<&str>) -> Result<bool> {

    let (where_clause, owned_params): (&str, Vec<Box<dyn ToSql>>) = match username {
        Some(u) => (
            "username = ?1 COLLATE NOCASE",
            vec![Box::new(u.to_string()) as Box<dyn ToSql>],
        ),
        None => ("username IS NULL", Vec::new()),
    };

    // Borrow them as &dyn ToSql
    let _param_refs: Vec<&dyn ToSql> = owned_params.iter().map(|b| b.as_ref()).collect();



    let query = format!(
        "SELECT failed_attempts, is_locked, locked_until FROM session_state WHERE {where_clause}"
    );

    let row: Option<(i64, i64, Option<String>)> =
        conn.query_row(&query, params_from_iter(owned_params), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;

    if let Some((fails, locked, until)) = row {
        let now = chrono::Utc::now();

        //Currently locked
        if locked == 1 {
            if let Some(u) = until {
                let until_time = chrono::DateTime::parse_from_rfc3339(&u)?.with_timezone(&chrono::Utc);
                if now < until_time {
                    let remaining = (until_time - now).num_seconds();
                    println!("Session temporarily locked. Try again in {remaining}s.");

                    log_event(conn, username.unwrap_or("<anonymous>"), username, "SESSION_LOCKOUT", Some(&format!("Session still locked.")))?;
                    return Ok(true);
                }
            }

            // Expired lock → clear
            conn.execute(
                "UPDATE session_state SET is_locked = 0, failed_attempts = 0, locked_until = NULL WHERE username IS NULL",
                [],
            )?;
            return Ok(false);
        }

        // Too many failed attempts
        if fails >= MAX_ATTEMPTS {
            let until = (now + chrono::Duration::seconds(SESSION_LOCK_SECONDS)).to_rfc3339();
            conn.execute(
                "UPDATE session_state SET is_locked = 1, locked_until = ?1 WHERE username IS NULL",
                params![until],
            )?;
            println!(
                "Too many failed attempts. Session locked for {} seconds.",
                SESSION_LOCK_SECONDS
            );
            log_event(conn, username.unwrap_or("<anonymous>"), username, "SESSION_LOCKOUT", Some(&format!("Session locked due to multiple fail attempts.")))?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn clear_lockout(conn: &Connection, current_admin: &str, username: Option<&str>) -> Result<()> {
    // Restrict access — only admins can clear or view lockouts
    if current_admin != "admin" {
        return Err(anyhow!("Access denied: only admin can view or clear lockouts."));
    }

    // Ensure the lockouts table exists
    // This prevents runtime panics if the database is new or not initialized yet.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS lockouts (
            username TEXT PRIMARY KEY COLLATE NOCASE,
            locked_until TEXT NOT NULL,
            lock_count INTEGER DEFAULT 1
        );",
        [],
    )
    .context("Failed to ensure lockouts table exists")?;

    //prepare SQL to read all locked accounts
    // The lockouts table stores who is locked, until when, and how many times.
    let mut stmt = conn
        .prepare(
            "SELECT username, locked_until, lock_count FROM lockouts ORDER BY locked_until ASC",
        )
        .context("Failed to query lockouts")?;

    //Execute the query and map results into Rust tuples
    // Each row will become (username, locked_until, lock_count)
    let lockouts = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?, // username
                r.get::<_, String>(1)?, // locked_until
                r.get::<_, i64>(2)?,    // lock_count
            ))
        })
        .context("Failed to iterate lockout rows")?;

    // Print all currently locked accounts
    println!("\nCurrently Locked Accounts:");
    let mut found_any = false; // track if any results were printed

    for row in lockouts {
        let (user, until, count) = row?; // unpack row data

        // Convert RFC3339 timestamp → Eastern Time (human-readable)
        let until_dt = DateTime::parse_from_rfc3339(&until)?.with_timezone(&New_York);

        // Print one lockout entry per line
        println!(
            " - {} | locked until {} | attempts escalated: {}x",
            user,
            until_dt.format("%Y-%m-%d %H:%M:%S %Z"),
            count
        );

        found_any = true;
    }

    // If there are no lockouts, exit
    if !found_any {
        println!("No users are currently locked out.\n");
        return Ok(());
    }

    // Optionally clear one specific user if username is provided
    if let Some(target_user) = username {
        // Delete the user’s entry from lockouts (if it exists)
        let affected = conn
            .execute(
                "DELETE FROM lockouts WHERE username = ?1 COLLATE NOCASE",
                params![target_user],
            )
            .context("Failed to delete lockout entry")?;

        //Report what happened
        if affected > 0 {
            // Found and cleared the lockout successfully
            println!("Lockout cleared for '{}'.", target_user);
            log_event(conn, current_admin, Some(target_user), "LOCKOUT_CLEARED", Some("Admin cleared lockout"))?;
        } else {
            // No matching entry found for that username
            println!("'{}' was not found among locked accounts.", target_user);
        }
    } else {
        // No username provided → only list lockouts without clearing anything
        println!("No username specified — no lockouts were cleared.");
    }

    Ok(())
}

pub fn view_security_log(conn: &Connection, _admin_username: &str, current_role: &str) -> Result<()> {
    // Ensure admin privileges
    if current_role != "admin" {
        println!("Access denied: Only administrators can view the security log.");
        return Ok(());
    }

    // Verify the table exists first to avoid panics
    let table_exists: bool = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='security_log'",
            [],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);

    if !table_exists {
        eprintln!("No security log found — the table 'security_log' does not exist.");
        return Ok(());
    }

    println!("\n===== Security Audit Log =====");
    println!("Filter options:");
    println!("  [1] View all logs");
    println!("  [2] Filter by username");
    println!("  [3] Filter by event type");
    println!("  [4] Show recent N entries");
    println!("  [0] Cancel");

    print!("Select an option: ");
    io::stdout().flush().ok();

    let mut choice = String::new();
    io::stdin().read_line(&mut choice).ok();
    let choice = choice.trim();

    // Start query
    let mut query = String::from(
        "SELECT timestamp, actor_username, target_username, event_type, description
         FROM security_log",
    );

    // For parameters
    let mut params_vec: Vec<String> = Vec::new();
    let mut limit_val: Option<i64> = None;

    match choice {
        "1" => {
            // no filters
        }
        "2" => {
            print!("Enter username to filter by (actor or target): ");
            io::stdout().flush().ok();
            let mut name = String::new();
            io::stdin().read_line(&mut name)?;
            let name = name.trim().to_string();
            query.push_str(" WHERE actor_username = ?1 OR target_username = ?1 COLLATE NOCASE");
            params_vec.push(name);
        }
        "3" => {
            print!("Enter event type (SUCCESS, FAILURE, LOCKOUT, etc.): ");
            io::stdout().flush().ok();
            let mut event = String::new();
            io::stdin().read_line(&mut event)?;
            let event = event.trim().to_uppercase();
            query.push_str(" WHERE event_type = ?1");
            params_vec.push(event);
        }
        "4" => {
            print!("Enter number of recent entries to view: ");
            io::stdout().flush().ok();
            let mut limit = String::new();
            io::stdin().read_line(&mut limit)?;
            let limit = limit.trim().parse::<i64>().unwrap_or(20);
            query.push_str(" ORDER BY id DESC LIMIT ?1");
            limit_val = Some(limit);
        }
        "0" => {
            println!("Cancelled viewing logs.");
            return Ok(());
        }
        _ => {
            println!("Invalid choice. Returning to admin menu.");
            return Ok(());
        }
    }

    query.push_str(";");

    let mut stmt = conn.prepare(&query)?;

    // Dynamically build SQL parameters (safe against SQL injection, no lifetimes)
    let mut params_boxed: Vec<Box<dyn ToSql>> = Vec::new();

    if !params_vec.is_empty() {
    params_boxed.push(Box::new(params_vec[0].clone()));
    } else if let Some(limit) = limit_val {
    params_boxed.push(Box::new(limit));
    }

    let params: Vec<&dyn ToSql> = params_boxed.iter().map(|b| b.as_ref()).collect();


    //Single query_map call with unified closure (no type mismatch)
    let rows = stmt.query_map(&*params, |r| {
        Ok((
            r.get::<_, String>(0)?,              // timestamp
            r.get::<_, String>(1)?,              // actor_username
            r.get::<_, Option<String>>(2)?,      // target_username
            r.get::<_, String>(3)?,              // event_type
            r.get::<_, Option<String>>(4)?,      // description
        ))
    })?;

    println!("\n{:<45} {:<15} {:<15} {:<18} {}", 
        "Timestamp (UTC)", "Actor", "Target", "Event", "Description");
    println!("{}", "-".repeat(130));

    let mut found_any = false;

    for row in rows {
        let (ts, actor, target, event, desc) = row?;
        found_any = true;
        println!(
            "{:<45} {:<15} {:<15} {:<18} {}",
            ts,
            actor,
            target.unwrap_or_else(|| "-".to_string()),
            event,
            desc.unwrap_or_else(|| "".to_string())
        );
    }

    if !found_any {
        println!("(No matching records found.)");
    }

    println!("{}", "-".repeat(130));
    Ok(())
}

