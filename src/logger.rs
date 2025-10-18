use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::America::New_York;
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs::OpenOptions, io::Write, thread, time::Duration as StdDuration};

// ------------------ PARAMETERS ------------------
const MAX_ATTEMPTS: usize = 5;          // Max failed attempts before lockout
const LOCKOUT_SECONDS_BASE: i64 = 30;   // Initial lockout (30s)
const MAX_LOCKOUT_SECONDS: i64 = 300;   // Max lockout cap (5 minutes)

// ------------------ HELPERS ------------------

// Current timestamp in Eastern Time (EST/EDT)
fn now_est() -> DateTime<chrono_tz::Tz> {
    New_York.from_utc_datetime(&Utc::now().naive_utc())
}

// Small random delay to prevent timing attacks (no SystemTime unwraps!)
pub fn fake_verification_delay() {
    let delay_ms: u64 = rand::thread_rng().gen_range(100..=250);
    thread::sleep(StdDuration::from_millis(delay_ms));
}

// ------------------ DATABASE SETUP ------------------

// Initialize logger database (no admin restriction — system needs it)
pub fn init_logger_db() -> Result<Connection> {
    let conn = Connection::open("logger.db").context("Failed to open logger.db")?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS login_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            status TEXT CHECK(status IN ('SUCCESS','FAILURE','LOCKOUT','LOCKOUT_CLEARED')) NOT NULL,
            timestamp TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS lockouts (
            username TEXT PRIMARY KEY,
            locked_until TEXT NOT NULL,
            lock_count INTEGER DEFAULT 1
        );
        "#,
    )?;
    Ok(conn)
}

// ------------------ LOGGING ------------------

// Log event to both DB and file
fn log_event(conn: &Connection, username: &str, event: &str) -> Result<()> {
    let timestamp = now_est().to_rfc3339();

    conn.execute(
        "INSERT INTO login_log (username, status, timestamp)
         VALUES (?1, ?2, ?3)",
        params![username, event, timestamp],
    )?;

    // Log to file
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("logins.log")
        .context("Failed to open logins.log")?;
    writeln!(file, "{} | {} | {}", timestamp, username, event)?;
    Ok(())
}

// ------------------ LOCKOUT SYSTEM ------------------

// Check if user is currently locked out
pub fn check_lockout(conn: &Connection, username: &str) -> Result<bool> {
    if let Some(locked_until_str) = conn
        .query_row(
            "SELECT locked_until FROM lockouts WHERE username = ?1",
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
                "⏳ Account '{}' is locked for another {} seconds (until {}).",
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
pub fn record_login_attempt(conn: &Connection, username: &str, success: bool) -> Result<()> {
    if success {
        log_event(conn, username, "SUCCESS")?;
        conn.execute("DELETE FROM lockouts WHERE username = ?1", params![username])?;
        return Ok(());
    }

    // Failed attempt
    log_event(conn, username, "FAILURE")?;

    // Count recent failures within last 5 minutes
    let recent_failures: i64 = conn.query_row(
        "SELECT COUNT(*) FROM login_log
         WHERE username = ?1 AND status = 'FAILURE'
         AND timestamp > datetime('now', '-5 minutes')",
        params![username],
        |r| r.get(0),
    )?;

    if recent_failures >= MAX_ATTEMPTS as i64 {
        // Get previous lockout count (if exists)
        let prev_count: Option<i64> = conn
            .query_row(
                "SELECT lock_count FROM lockouts WHERE username = ?1",
                params![username],
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
            params![username, locked_until, next_count],
        )?;

        log_event(conn, username, "LOCKOUT")?;
        println!(
            "'{}' locked for {} seconds (until {}).",
            username, lockout_secs, locked_until
        );
    }

    Ok(())
}

// Allow admin to clear lockouts (but log the action)
pub fn clear_lockout(conn: &Connection, current_role: &str, username: &str) -> Result<()> {
    if current_role != "admin" {
        return Err(anyhow!("Access denied: only admin can remove lockouts."));
    }

    let affected = conn.execute("DELETE FROM lockouts WHERE username = ?1", params![username])?;
    if affected > 0 {
        println!("Admin cleared active lockout for '{}'.", username);
        log_event(conn, username, "LOCKOUT_CLEARED")?;
    } else {
        println!("No active lockout found for '{}'.", username);
    }
    Ok(())
}
