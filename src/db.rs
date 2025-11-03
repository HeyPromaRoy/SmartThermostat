use anyhow::{anyhow, Context, Result};
use base64::{Engine, engine::general_purpose};
use blake3;
use chrono::{DateTime, Utc, NaiveDateTime};
use chrono_tz::America::New_York;
use rusqlite::{params, Connection, OptionalExtension};
use rpassword::read_password;
use rand::{TryRngCore, rngs::OsRng};
use std::{io::{self, Write}, path::Path};
use zeroize::Zeroizing;

use crate::auth;
use crate::logger;
use crate::weather::WeatherRecord;

// Converts UTC timestamp strings (e.g. "2025-10-18 13:32:39") into America/New_York time (EDT/EST).
fn to_eastern_time(utc_str: &str) -> Option<String> {
    if let Ok(naive) = NaiveDateTime::parse_from_str(utc_str, "%Y-%m-%d %H:%M:%S") {
        let utc_dt: DateTime<Utc> = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        let eastern_dt = utc_dt.with_timezone(&New_York);
        Some(eastern_dt.format("%Y-%m-%d %H:%M:%S %Z").to_string())
    } else {
        None
    }
}

// Initialize all required database tables and indexes.
pub fn init_system_db<P: AsRef<Path>>(db_path: P) -> Result<Connection> {
    let conn = Connection::open(db_path).context("Failed to open db")?;
        //Apply secure PRAGMA settings
        conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=FULL;
        PRAGMA foreign_keys=ON;
        PRAGMA secure_delete=ON;
        PRAGMA temp_store=MEMORY;
        "#,
    )
    .context("Failed to apply secure PRAGMA settings")?;

    // create tables for users, security logs, and lockouts
    conn.execute_batch(
        r#"
        -- ===============================
        -- USERS TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS users (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            username        TEXT NOT NULL UNIQUE COLLATE NOCASE,
            hashed_password TEXT NOT NULL,
            user_status     TEXT CHECK(user_status IN ('admin', 'technician', 'homeowner','guest')) NOT NULL,
            homeowner_id    INTEGER REFERENCES users(id),
            is_active       INTEGER DEFAULT 1,
            last_login_time TEXT,
            created_at      TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at      TEXT
        );

        CREATE INDEX IF NOT EXISTS ix_users_homeowner_id ON users(homeowner_id);
        CREATE INDEX IF NOT EXISTS ix_users_username ON users(username);

        -- ===============================
        -- SECURITY LOG TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS security_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            actor_username TEXT NOT NULL,
            target_username TEXT NOT NULL,
            event_type TEXT NOT NULL CHECK(
                event_type IN (
                    'ACCOUNT_CREATED', 'SUCCESS_LOGIN', 'FAILURE_LOGIN', 'LOGOUT', 'LOCKOUT', 'SESSION_LOCKOUT', 'LOCKOUT_CLEARED',
                    'ACCOUNT_DELETED', 'ACCOUNT_DISABLED', 'ACCOUNT_ENABLED', 'ADMIN_LOGIN', 'PASSWORD_CHANGE', 'HVAC',
                    'ACCESS_GRANTED', 'ACCESS_EXPIRED', 'TECH_ACCESS'
                )
            ),
            description TEXT,
            timestamp TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS ix_security_log_actor ON security_log(actor_username);
        CREATE INDEX IF NOT EXISTS ix_security_log_target ON security_log(target_username);

        -- ===============================
        -- LOCKOUT TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS lockouts (
            username TEXT PRIMARY KEY COLLATE NOCASE,
            locked_until TEXT NOT NULL,
            lock_count INTEGER DEFAULT 1
        );

        CREATE INDEX IF NOT EXISTS ix_lockouts_username ON lockouts(username);
        
        -- ===============================
        --      SESSION STATE TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS session_state (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT UNIQUE COLLATE NOCASE,
            session_token_hash TEXT UNIQUE,
            login_time TEXT DEFAULT CURRENT_TIMESTAMP,
            last_active_time TEXT,
            session_expires TEXT,
            failed_attempts INTEGER DEFAULT 0,
            is_locked INTEGER DEFAULT 0,
            locked_until TEXT,
            session_lock_count INTEGER DEFAULT 0,
            FOREIGN KEY(username) REFERENCES users(username) ON DELETE CASCADE ON UPDATE CASCADE
        );

        -- ===============================
        --      TECHNICIAN JOB TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS technician_jobs (
            job_id INTEGER PRIMARY KEY AUTOINCREMENT,
            homeowner_username  TEXT NOT NULL COLLATE NOCASE
                REFERENCES users(username) ON DELETE RESTRICT,
            technician_username TEXT NOT NULL COLLATE NOCASE
                REFERENCES users(username) ON DELETE RESTRICT,
            status TEXT NOT NULL
                CHECK (status IN ('ACCESS_GRANTED','TECH_ACCESS','ACCESS_EXPIRED')),
            access_minutes INTEGER NOT NULL
                CHECK (access_minutes IN (30,60,90,120)),

            grant_start   TEXT NOT NULL DEFAULT (datetime('now')),
            grant_expires TEXT GENERATED ALWAYS AS (
                datetime(grant_start, printf('+%d minutes', access_minutes))
                ) VIRTUAL,

            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

            job_desc TEXT NOT NULL
                CHECK (
                length(job_desc) BETWEEN 20 AND 200
                AND job_desc NOT LIKE '%' || char(10) || '%'
                AND job_desc NOT LIKE '%' || char(13) || '%'
                ),
            notes TEXT
        );

        CREATE INDEX IF NOT EXISTS ix_jobs_access
        ON technician_jobs (homeowner_username, technician_username, status, updated_at);

        -- ===============================
        -- WEATHER TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS weather (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            time TEXT,
            temperature_f REAL,
            temperature_c REAL,
            dewpoint_f REAL,
            dewpoint_c REAL,
            humidity REAL,
            wind_speed_mph REAL,
            wind_direction_deg REAL,
            condition TEXT
        );

        -- ===============================
        -- PROFILES TABLE (ADMIN-EDITABLE)
        -- ===============================
        CREATE TABLE IF NOT EXISTS profiles (
            name TEXT PRIMARY KEY,
            mode TEXT NOT NULL CHECK(mode IN ('Off','Heating','Cooling','FanOnly','Auto')),
            target_temp REAL NOT NULL,
            greeting TEXT,
            description TEXT,
            heater_status TEXT DEFAULT 'Auto' CHECK(heater_status IN ('On','Off','Auto')),
            ac_status TEXT DEFAULT 'Auto' CHECK(ac_status IN ('On','Off','Auto')),
            vacation_start_date TEXT,
            vacation_end_date TEXT,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        );

        -- ===============================
        -- HVAC ACTIVITY LOG TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS hvac_activity_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            user_role TEXT NOT NULL,
            action_type TEXT NOT NULL CHECK(
                action_type IN ('PROFILE_APPLIED', 'PROFILE_EDITED', 'PROFILE_RESET', 'TEMPERATURE_CHANGED', 'MODE_CHANGED')
            ),
            profile_name TEXT,
            old_value TEXT,
            new_value TEXT,
            description TEXT,
            timestamp TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS ix_hvac_log_username ON hvac_activity_log(username);
        CREATE INDEX IF NOT EXISTS ix_hvac_log_timestamp ON hvac_activity_log(timestamp);
        CREATE INDEX IF NOT EXISTS ix_hvac_log_action ON hvac_activity_log(action_type);

        -- ===============================
        -- HVAC SYSTEM STATE TABLE
        -- ===============================
        CREATE TABLE IF NOT EXISTS hvac_state (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            mode TEXT NOT NULL CHECK(mode IN ('Off','Heating','Cooling','FanOnly','Auto')),
            target_temperature REAL NOT NULL,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        );

        -- Initialize with default state if empty
        INSERT OR IGNORE INTO hvac_state (id, mode, target_temperature) VALUES (1, 'Off', 22.0);
        "#,
    )
    .context("Failed to initialize tables in system.db")?;

    // Migrate existing profiles table to add new columns if they don't exist
    migrate_profiles_table(&conn)?;
    
    // Migrate existing hvac_state table to add light_status if it doesn't exist
    migrate_hvac_state_table(&conn)?;
    
    // Migrate security_log table to add technician event types
    migrate_security_log_table(&conn)?;

    // Seed default profiles if missing
    seed_default_profiles(&conn)?;
    
    // Update Party profile to have light ON (fix for existing databases)
    conn.execute(
        "UPDATE profiles SET light_status = 'ON' WHERE name = 'Party' AND (light_status IS NULL OR light_status = 'OFF')",
        [],
    )?;

    Ok(conn)
}

// Returns a reusable SQLite connection to the unified database.
pub fn get_connection<P: AsRef<Path>>(db_path: P) -> Result<Connection> {
    init_system_db(db_path)
}

// Check if a username already exists.
pub fn user_exists(conn: &Connection, username: &str) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users WHERE username = ?1 COLLATE NOCASE",
            params![username],
            |r| r.get(0),
        )
        .context("Failed to query user existence")?;
    Ok(count > 0)
}

// Retrieve a user's ID and role by username
pub fn get_user_id_and_role(conn: &Connection, username: &str) -> Result<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, user_status FROM users WHERE username = ?1 COLLATE NOCASE",
            params![username],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?)
}

// Insert a new user record (used internally by registration).
pub fn insert_user(conn: &mut Connection, username: &str, admin_username: &str ,hashed: &str, role: &str, homeowner_id: Option<i64>) -> Result<()> {
    let tx = conn.transaction().context("Failed to start transaction")?;
    tx.execute(
        "INSERT INTO users (username, hashed_password, user_status, homeowner_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        params![username, hashed, role, homeowner_id],)
        .context("Failed to insert user")?;

        tx.commit().context("Failed to commit transaction")?;

     let desc = format!("User '{}' created by '{}'", username, admin_username);
    logger::log_event(conn, admin_username, Some(&username), "ACCOUNT_CREATED", Some(&desc))?;

    Ok(())
}


pub fn show_own_profile(conn: &Connection, username: &str) -> Result<()> {
    let row = conn.query_row(
        "SELECT id, username, user_status, created_at, last_login_time FROM users WHERE username = ?1 COLLATE NOCASE",
        params![username],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        },
    ).optional()?;

    if let Some((id, uname, role, created, last_login)) = row {
        println!("Viewing profile for {}...", username);
        println!("===== Profile =====");
        println!("ID: {}", id);
        println!("username: {}", uname);
        println!("Role: {}", role);
        
        // Convert UTC -> Eastern for display
        let created_str = created
            .as_deref()
            .and_then(to_eastern_time)
            .unwrap_or_else(|| "unknown".to_string());

        let last_login_str = last_login
            .as_deref()
            .and_then(to_eastern_time)
            .unwrap_or_else(|| "never".to_string());

        println!("Created: {}", created_str);
        println!("Last Login: {}", last_login_str);
    } else {
        println!("User '{}' not found.", username);
    }
    Ok(())
}

// List all guests registered under a given homeowner.
pub fn list_guests_of_homeowner(conn: &Connection, homeowner_username: &str) -> Result<()> {
    if let Some((homeowner_id, _)) = get_user_id_and_role(conn, homeowner_username)? {
        let mut stmt = conn
            .prepare(
                "SELECT username, created_at
                 FROM users
                 WHERE user_status = 'guest'
                 AND homeowner_id = ?1
                 ORDER BY created_at DESC",
            )
            .context("Failed to prepare guest list query")?;

        let guests = stmt
            .query_map(params![homeowner_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;

        println!("Guests for '{}':", homeowner_username);
        for g in guests {
            let (uname, created) = g?;
            println!(" - {} (created {})", uname, created);
        }
    } else {
        println!("Homeowner '{}' not found.", homeowner_username);
    }

    Ok(())
}


// View all registered users (admin-only)
pub fn view_all_users(conn: &Connection, current_role: &str) -> Result<()> {
    // Only allow admins to view this
    if current_role != "admin" {
        println!("Access denied: Only administrators can view all users.");
        return Ok(()); // Return silently — no panic, no data leak
    }

    // Prepare SQL query for user listing
    let mut stmt = conn.prepare(
        r#"
        SELECT id, username, user_status, is_active, created_at, last_login_time FROM users
        ORDER BY created_at ASC
        "#,
    ).context("Failed to prepare query for all users")?;

    let users = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,      // id
                row.get::<_, String>(1)?,   // username
                row.get::<_, String>(2)?,   // role
                row.get::<_, i64>(3)?,      // is_active
                row.get::<_, Option<String>>(4)?, // created_at
                row.get::<_, Option<String>>(5)?, // last_login_time
            ))
        })
        .context("Failed to query users")?;

    println!("\n===== Registered Users =====");
    println!("{:<5} {:<15} {:<12} {:<8} {:<25} {:<25}",
        "ID", "Username", "Role", "Active", "Created At (EST)", "Last Login (EST)");
    println!("{}", "-".repeat(95));

    for user in users {
        let (id, username, role, is_active, created_at, last_login_time) = user?;
        
        // convert UTC to EST
        let created_str = created_at
            .as_deref()
            .and_then(to_eastern_time)
            .unwrap_or_else(|| "unknown".to_string());

        let last_login_str = last_login_time
            .as_deref()
            .and_then(to_eastern_time)
            .unwrap_or_else(|| "never".to_string());
        let active_str = if is_active == 1 { "ACTIVE" } else { "INACTIVE" };

        println!(
            "{:<5} {:<15} {:<12} {:<8} {:<25} {:<25}",
            id, username, role, active_str, created_str, last_login_str
        );
    }

    println!("========================================\n");
    Ok(())
}



// Allows an admin to enable or disable user accounts.
pub fn manage_user_status(conn: &mut Connection, admin_username: &str, current_role: &str) -> Result<()> {
    if current_role != "admin" {
        println!("Access denied: Only admins can manage accounts.");
        return Ok(());
    }

    // Verify admin identity
    println!("\nAdmin re-authentication required.");
    print!("Enter your password: ");
    io::stdout().flush().ok();

    let admin_pw = {
        let raw = read_password()?;
        Zeroizing::new(raw.trim_end_matches(['\r', '\n']).to_string())
    };

    let stored_hash: Option<String> = conn
        .query_row(
            "SELECT hashed_password FROM users WHERE username = ?1 COLLATE NOCASE",
            params![admin_username],
            |r| r.get(0),
        )
        .optional()?;

    let ok = stored_hash
        .as_ref()
        .map(|h| auth::verify_password(&admin_pw, h).unwrap_or(false))
        .unwrap_or(false);

    if !ok {
        println!("Authentication failed. Aborting.");
        return Ok(());
    }

    // List all users
    println!("\n===== User Management =====");
    let mut stmt = conn.prepare("SELECT id, username, user_status, is_active FROM users ORDER BY username")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;

    println!("{:<5} {:<15} {:<12} {:<10}", "ID", "Username", "Role", "Status");
    println!("{}", "-".repeat(45));
    for row in rows {
        let (id, user, role, active): (i64, String, String, i64) = row?;
        println!("{:<5} {:<15} {:<12} {:<10}", id, user, role, if active == 1 { "Active" } else { "Disabled" });
    }

    print!("\nEnter username or ID to toggle (or 'cancel' to exit): ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let input = input.trim();

    if input.eq_ignore_ascii_case("cancel") {
        println!("Operation cancelled.");
        return Ok(());
    }

    let row = conn.query_row(
        "SELECT id, username, user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE OR id = CAST(?1 AS INTEGER)",
        params![input],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)),
    );

    let (user_id, target_username, target_role, is_active) = match row {
        Ok(v) => v,
        Err(_) => {
            println!("User '{}' not found.", input);
            return Ok(());
        }
    };

    // Protection: don't disable self or another admin
    if target_username.eq_ignore_ascii_case(admin_username) {
        println!("You cannot disable your own account.");
        return Ok(());
    }
    if target_role == "admin" {
        println!("You cannot modify another admin’s account.");
        return Ok(());
    }

    let new_status = if is_active == 1 { 0 } else { 1 };
    let action = if new_status == 1 { "enabled" } else { "disabled" };

    conn.execute(
        "UPDATE users SET is_active = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![new_status, user_id],
    )?;

    println!("User '{}' has been {}.", target_username, action);

    let event_type = if new_status == 1 { "ACCOUNT_ENABLED" } else { "ACCOUNT_DISABLED" };
    let desc = format!("User '{}' {} by Admin '{}'", target_username, action, admin_username);

    logger::log_event(conn, admin_username, Some(&target_username), event_type, Some(&desc))?;
    Ok(())
}

// ======================================================
//                   TECHNICIANS
// ======================================================

pub fn grant_technician_access(conn: &mut Connection, 
    homeowner_username: &str, technician_username: &str, 
    access_minutes: i64, job_desc_raw: &str) -> Result<i64> {
    
        //validate access time
    if ![30, 60, 90, 120].contains(&access_minutes) {
        return Err(anyhow!("Invalid access time; must be one of 30, 60, 90, 120."));
    }
    
    // sanitize and length bounds
    let mut desc = job_desc_raw.trim().to_string();
    desc.retain(|c| !c.is_control());
    let desc = desc.split_whitespace().collect::<Vec<_>>().join(" ");
    if desc.is_empty() {
        return Err(anyhow!("Description cannot be empty."));
    }
    let len = desc.chars().count();
    if len < 20 || len > 200 {
        return Err(anyhow!("Description must be 20–200 characters (current: {}).", len));
    }
    
    //validation of actors and their roles
    let (h_role, h_active): (String, i64) = conn.query_row(
        "SELECT user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE",
        params![homeowner_username],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).context("Failed to fetch homeowner record")?;

    if h_role != "homeowner" || h_active != 1 {
        return Err(anyhow!("Invalid homeowner account or account is inactive."));
    }

    let (t_role, t_active): (String, i64) = conn.query_row(
        "SELECT user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE",
        params![technician_username],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).context("Failed to fetch technician record")?;

    if t_role != "technician" || t_active != 1 {
        return Err(anyhow!("Invalid technician account or account is inactive."));
    }

  // insert & transaction
    let job_id = {
        let tx = conn.transaction()?;
        tx.execute(
            r#"
            INSERT INTO technician_jobs
                (homeowner_username, technician_username, status, access_minutes, job_desc, updated_at)
            VALUES (?1, ?2, 'ACCESS_GRANTED', ?3, ?4, datetime('now'))
            "#,
            // PASS &desc so it is NOT moved and can be reused below
            params![homeowner_username, technician_username, access_minutes, &desc],
        )?;
        let jid = tx.last_insert_rowid();
        tx.commit()?; // <-- commit before logging to avoid E0502
        jid
    };
    
    if let Err(e) = logger::log_event(conn, homeowner_username, Some(technician_username), "ACCESS_GRANTED",
        Some(&format!("job_id={}, minutes={}, desc={}", job_id, access_minutes, desc)),
    ) {
        eprintln!("(log_event failed: {e})");
    }

    Ok(job_id)
}


pub fn access_job(conn: &Connection, job_id: i64, technician_username: &str) -> Result<Option<(String, String, String)>> {
    // Load & validate ownership
    let row = conn
        .query_row(
            r#"
            SELECT homeowner_username, job_desc, grant_expires, status
              FROM technician_jobs
             WHERE job_id = ?1
               AND technician_username = ?2 COLLATE NOCASE
            "#,
            params![job_id, technician_username],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        )
        .optional()?
        .ok_or_else(|| anyhow!("Job not found or not assigned to you"))?;
    let (homeowner, desc, expires, _status) = row;

    // Try to claim TECH_ACCESS if still valid
    let claimed = conn.execute(
        r#"
        UPDATE technician_jobs
           SET status = 'TECH_ACCESS', updated_at = datetime('now')
         WHERE job_id = ?1
           AND technician_username = ?2 COLLATE NOCASE
           AND status IN ('ACCESS_GRANTED','TECH_ACCESS')
           AND grant_expires > datetime('now')
        "#,
        params![job_id, technician_username],
    )?;
    if claimed > 0 {
        let _ = crate::logger::log_event(
            conn, technician_username, Some(&homeowner),
            "TECH_ACCESS", Some(&format!("job_id={} | desc={}", job_id, desc)),
        );
        return Ok(Some((homeowner, desc, expires)));
    }

    // Otherwise flip to ACCESS_EXPIRED if it really is expired now
    let flipped = conn.execute(
        r#"
        UPDATE technician_jobs
           SET status = 'ACCESS_EXPIRED', updated_at = datetime('now')
         WHERE job_id = ?1
           AND status IN ('ACCESS_GRANTED','TECH_ACCESS')
           AND grant_expires <= datetime('now')
        "#,
        params![job_id],
    )?;
    if flipped > 0 {
        let _ = crate::logger::log_event(
            conn, technician_username, Some(&homeowner),
            "ACCESS_EXPIRED", Some(&format!("job_id={}", job_id)),
        );
    }

    Ok(None)
}

// Flip any grants that have expired right now to ACCESS_EXPIRED.
// Returns how many rows were updated. Best-effort logs per row.
pub fn sweep_expire_grants(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        r#"
        UPDATE technician_jobs
           SET status = 'ACCESS_EXPIRED',
               updated_at = datetime('now')
         WHERE status IN ('ACCESS_GRANTED', 'TECH_ACCESS')
           AND grant_expires <= datetime('now')
         RETURNING job_id, homeowner_username, technician_username
        "#,
    )?;

    let mut rows = stmt.query([])?;
    let mut changed = 0usize;

    while let Some(r) = rows.next()? {
        let job_id: i64 = r.get(0)?;
        let homeowner_username: String = r.get(1)?;
        let technician_username: String = r.get(2)?;
        changed += 1;

        // Best-effort logging; don't fail the sweep if logs fail
        let _ = crate::logger::log_event(
            conn,
            &technician_username,
            Some(&homeowner_username),
            "ACCESS_EXPIRED",
            Some(&format!("job_id={}", job_id)),
        );
    }

    Ok(changed)
}


pub fn tech_has_perm(conn: &Connection, acting_username: &str, homeowner_username: &str) -> Result<bool> {
    
    let _ = crate::db::sweep_expire_grants(conn);

    //fetch roles/enabled once
    let target: Option<(String, i64)> = conn
        .query_row(
            "SELECT user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE",
            params![homeowner_username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional()?;

    let Some((target_role, target_active)) = target else {
        // Unknown target -> deny
        return Ok(false);
    };
    
    if target_role != "homeowner" || target_active != 1 {
        // Not a live homeowner account -> deny
        return Ok(false);
    }
    let actor: Option<(String, i64)> = conn
        .query_row(
            "SELECT user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE",
            params![acting_username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    let Some((actor_role, actor_active)) = actor else { return Ok(false); };
    
    if actor_active != 1 { return Ok(false); }

    // Admins can act on anyone
    if actor_role == "admin" { return Ok(true);}

    // A homeowner can act on themselves
    if actor_role == "homeowner" && acting_username.eq_ignore_ascii_case(homeowner_username)
    { return Ok(true); }

    // Technicians need an active, time-boxed grant
    if actor_role == "technician" {
        // Reuse the same grant logic you use elsewhere
        let ok: Option<i64> = conn
            .query_row(
                r#"
                SELECT 1
                  FROM technician_jobs
                 WHERE technician_username = ?1 COLLATE NOCASE AND homeowner_username  = ?2 COLLATE NOCASE AND status IN ('ACCESS_GRANTED','TECH_ACCESS')
                   AND datetime(updated_at, printf('+%d minutes', access_minutes)) > datetime('now')
                 LIMIT 1
                "#,
                params![acting_username, homeowner_username],
                |r| r.get(0),
            )
            .optional()?;
        return Ok(ok.is_some());
    }

    // Guests or other roles: deny
    Ok(false)
}

// List all active technician assignments
pub fn list_active_grants(conn: &Connection, username: &str) -> Result<()> {
    
    let _ = crate::db::sweep_expire_grants(conn);

    let mut stmt = conn.prepare(
        r#"
        SELECT job_id, homeowner_username, technician_username, status, grant_start, grant_expires, access_minutes
        FROM technician_jobs
        WHERE (homeowner_username = ?1 COLLATE NOCASE OR technician_username = ?1 COLLATE NOCASE)
          AND grant_expires > datetime('now')
          AND status = 'ACCESS_GRANTED'
        ORDER BY grant_expires DESC
        "#
    )?;
    let mut rows = stmt.query(params![username])?;
    println!("Active grants visible to '{}':", username);
    println!("{:<8} {:<15} {:<15} {:<12} {:<20} {:<20} {:<5}",
        "job_id","homeowner","technician","status","start","expires","mins");
    while let Some(r) = rows.next()? {
        let (jid,h,t,st,gs,ge,m):(i64,String,String,String,String,String,i64) =
            (r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?);
        println!("{:<8} {:<15} {:<15} {:<12} {:<20} {:<20} {:<5}", jid,h,t,st,gs,ge,m);
    }
    Ok(())
}


// ======================================================
//                          TOKEN
// ======================================================

fn new_session_token() -> (Zeroizing<String>, String) {
    let mut buf = [0u8; 32];
    let mut rng = OsRng;
    TryRngCore::try_fill_bytes(&mut rng, &mut buf).expect("OS RNG failed");


    // Plain token returned to caller (for headers/cookies/in-memory)
    let token_plain = Zeroizing::new(general_purpose::URL_SAFE_NO_PAD.encode(buf));
    
    // Hash stored in DB (hex)
    let token_hash_hex = blake3::hash(&buf).to_hex().to_string();
    // Wipe the temporary random buffer
    buf.fill(0);

    (token_plain, token_hash_hex)
}

/*Creates or updates a user session in the `session_state` table.
    - Marks old sessions as inactive
    - Inserts a new session record with expiry 30 min from now
    - Returns the session_token for in-memory tracking */
pub fn update_session(conn: &Connection, username: Option<&str>) -> Result<String> {
    let where_clause = if username.is_some() {
        "username = ?1"
    } else {
        "username IS NULL"
    };

    // Delete only expired sessions
    if let Some(u) = username {
    conn.execute(
        &format!(
            "DELETE FROM session_state WHERE {} AND session_expires <= datetime('now')",
            where_clause
        ),
    params![u],
    )?;
    } else {
    conn.execute(
        &format!(
            "DELETE FROM session_state WHERE {} AND session_expires <= datetime('now')",
            where_clause
        ),
        [], // no parameters
    )?;
    }

    // Check if an active session already exists
    let has_live_session: Option<i64> = if let Some(u) = username {
        conn.query_row(
            &format!(
                "SELECT 1 FROM session_state WHERE {} AND session_expires > datetime('now') LIMIT 1",
                where_clause
            ),
            rusqlite::params![u],
            |r| r.get(0),
        ).optional()?
    } else {
        conn.query_row(
            &format!(
                "SELECT 1 FROM session_state WHERE {} AND session_expires > datetime('now') LIMIT 1",
                where_clause
            ),
            [],
            |r| r.get(0),
        ).optional()?
    };

    //Refresh expiry if session already exists
    if has_live_session.is_some() {
        if let Some(u) = username {
            conn.execute(
                &format!(
                    "UPDATE session_state SET session_expires = datetime('now', '+10 minutes') WHERE {}",
                    where_clause
                ),
                rusqlite::params![u],
            )?;
        } else {
            conn.execute(
                &format!(
                    "UPDATE session_state SET session_expires = datetime('now', '+10 minutes') WHERE {}",
                    where_clause
                ),
                [],
            )?;
        }
        return Ok("<existing-session>".to_string());
    }

    // Generate a new token
    let (token_plain, token_hash_hex) = new_session_token();

    let expires = Utc::now() + chrono::Duration::minutes(10);
    let expires_str = expires.format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO session_state 
         (username, session_token_hash, login_time, session_expires, failed_attempts, is_locked)
         VALUES (?1, ?2, datetime('now'), ?3, 0, 0)",
        params![username, token_hash_hex, expires_str],
    )?;

    Ok(token_plain.to_string())
}

// Delete the active session of the user
pub fn end_session(conn: &Connection, username: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM session_state WHERE username = ?1", params![username])?;
    Ok(())
}

// ======================================================
//                     PROFILES (HVAC)
// ======================================================

#[derive(Debug, Clone)]
pub struct ProfileRow {
    pub name: String,
    pub mode: String,
    pub target_temp: f32,
    pub greeting: Option<String>,
    pub description: Option<String>,
    pub heater_status: String,
    pub ac_status: String,
    pub light_status: String,
    pub fan_speed: String,
    #[allow(dead_code)]
    pub vacation_start_date: Option<String>,
    #[allow(dead_code)]
    pub vacation_end_date: Option<String>,
}

fn default_profile_row(name: &str) -> Option<ProfileRow> {
    match name {
        "Day" => Some(ProfileRow {
            name: "Day".to_string(),
            mode: "Auto".to_string(),
            target_temp: 22.0,
            greeting: Some("☀️ Hope you have a good day!".to_string()),
            description: Some("Auto mode, comfort-oriented, 21-23°C / 24-26°C, Medium fan, Comfort".to_string()),
            heater_status: "Auto".to_string(),
            ac_status: "Auto".to_string(),
            light_status: "OFF".to_string(),
            fan_speed: "Medium".to_string(),
            vacation_start_date: None,
            vacation_end_date: None,
        }),
        "Night" => Some(ProfileRow {
            name: "Night".to_string(),
            mode: "Auto".to_string(),
            target_temp: 20.0,
            greeting: Some("🌙 Have a Good Night!".to_string()),
            description: Some("Auto or steady heating/cooling, 20°C heating / 25°C cooling, Low fan speed, Moderate".to_string()),
            heater_status: "Auto".to_string(),
            ac_status: "Auto".to_string(),
            light_status: "ON".to_string(),
            fan_speed: "Low".to_string(),
            vacation_start_date: None,
            vacation_end_date: None,
        }),
        "Sleep" => Some(ProfileRow {
            name: "Sleep".to_string(),
            mode: "Heating".to_string(),
            target_temp: 25.0,
            greeting: Some("😴 Sleep well and sweet dreams!".to_string()),
            description: Some("Heating preferred, quiet fan, 18-20°C heating / 26-28°C cooling, Low fan, Energy saving".to_string()),
            heater_status: "On".to_string(),
            ac_status: "Off".to_string(),
            light_status: "OFF".to_string(),
            fan_speed: "Low".to_string(),
            vacation_start_date: None,
            vacation_end_date: None,
        }),
        "Party" => Some(ProfileRow {
            name: "Party".to_string(),
            mode: "Cooling".to_string(),
            target_temp: 20.0,
            greeting: Some("🎊 Let's get this party started!".to_string()),
            description: Some("Cooling with ventilation, 22°C heating / 23-24°C cooling, High fan, Comfort prioritized".to_string()),
            heater_status: "Off".to_string(),
            ac_status: "On".to_string(),
            light_status: "ON".to_string(),
            fan_speed: "High".to_string(),
            vacation_start_date: None,
            vacation_end_date: None,
        }),
        "Vacation" => Some(ProfileRow {
            name: "Vacation".to_string(),
            mode: "Off".to_string(),
            target_temp: 24.0,
            greeting: Some("🏖️ Enjoy your vacation!".to_string()),
            description: Some("HVAC mostly off, 16-18°C heating / 29-30°C cooling, Fan off, Max energy saving".to_string()),
            heater_status: "Off".to_string(),
            ac_status: "Off".to_string(),
            light_status: "OFF".to_string(),
            fan_speed: "Low".to_string(),
            vacation_start_date: None,
            vacation_end_date: None,
        }),
        "Away" => Some(ProfileRow {
            name: "Away".to_string(),
            mode: "Off".to_string(),
            target_temp: 25.0,
            greeting: Some("🚗 Have a safe trip!".to_string()),
            description: Some("HVAC off/eco mode, 25°C / 77°F, Fan off, Energy saving".to_string()),
            heater_status: "Off".to_string(),
            ac_status: "Off".to_string(),
            light_status: "OFF".to_string(),
            fan_speed: "Low".to_string(),
            vacation_start_date: None,
            vacation_end_date: None,
        }),
        _ => None,
    }
}

fn migrate_profiles_table(conn: &Connection) -> Result<()> {
    // Check if heater_status column exists
    let column_check: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('profiles') WHERE name='heater_status'",
        [],
        |r| r.get(0),
    );
    
    if let Ok(count) = column_check {
        if count == 0 {
            // Recreate table with new columns (no ALTER TABLE)
            conn.execute_batch(
                r#"
                -- Create new table with all columns
                CREATE TABLE profiles_new (
                    name TEXT PRIMARY KEY,
                    mode TEXT NOT NULL CHECK(mode IN ('Off','Heating','Cooling','FanOnly','Auto')),
                    target_temp REAL NOT NULL,
                    greeting TEXT,
                    description TEXT,
                    heater_status TEXT DEFAULT 'Auto' CHECK(heater_status IN ('On','Off','Auto')),
                    ac_status TEXT DEFAULT 'Auto' CHECK(ac_status IN ('On','Off','Auto')),
                    vacation_start_date TEXT,
                    vacation_end_date TEXT,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                );

                -- Copy existing data from old table
                INSERT INTO profiles_new (name, mode, target_temp, greeting, description, heater_status, ac_status, vacation_start_date, vacation_end_date, updated_at)
                SELECT 
                    name, 
                    mode, 
                    target_temp, 
                    greeting, 
                    description,
                    'Auto' as heater_status,
                    'Auto' as ac_status,
                    NULL as vacation_start_date,
                    NULL as vacation_end_date,
                    updated_at
                FROM profiles;

                -- Drop old table
                DROP TABLE profiles;

                -- Rename new table to original name
                ALTER TABLE profiles_new RENAME TO profiles;
                "#
            )?;
        }
    }
    
    // Check if light_status column exists
    let light_column_check: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('profiles') WHERE name='light_status'",
        [],
        |r| r.get(0),
    );
    
    if let Ok(count) = light_column_check {
        if count == 0 {
            // Recreate table again to add light_status column
            conn.execute_batch(
                r#"
                -- Create new table with light_status column
                CREATE TABLE profiles_new (
                    name TEXT PRIMARY KEY,
                    mode TEXT NOT NULL CHECK(mode IN ('Off','Heating','Cooling','FanOnly','Auto')),
                    target_temp REAL NOT NULL,
                    greeting TEXT,
                    description TEXT,
                    heater_status TEXT DEFAULT 'Auto' CHECK(heater_status IN ('On','Off','Auto')),
                    ac_status TEXT DEFAULT 'Auto' CHECK(ac_status IN ('On','Off','Auto')),
                    light_status TEXT DEFAULT 'OFF' CHECK(light_status IN ('ON','OFF')),
                    vacation_start_date TEXT,
                    vacation_end_date TEXT,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                );

                -- Copy all existing data
                INSERT INTO profiles_new (name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, vacation_start_date, vacation_end_date, updated_at)
                SELECT 
                    name, 
                    mode, 
                    target_temp, 
                    greeting, 
                    description,
                    heater_status,
                    ac_status,
                    'OFF' as light_status,
                    vacation_start_date,
                    vacation_end_date,
                    updated_at
                FROM profiles;

                -- Drop old table
                DROP TABLE profiles;

                -- Rename new table
                ALTER TABLE profiles_new RENAME TO profiles;
                "#
            )?;
        }
    }
    
    // Check if fan_speed column exists
    let fan_column_check: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('profiles') WHERE name='fan_speed'",
        [],
        |r| r.get(0),
    );
    
    if let Ok(count) = fan_column_check {
        if count == 0 {
            // Recreate table again to add fan_speed column
            conn.execute_batch(
                r#"
                -- Create new table with fan_speed column
                CREATE TABLE profiles_new (
                    name TEXT PRIMARY KEY,
                    mode TEXT NOT NULL CHECK(mode IN ('Off','Heating','Cooling','FanOnly','Auto')),
                    target_temp REAL NOT NULL,
                    greeting TEXT,
                    description TEXT,
                    heater_status TEXT DEFAULT 'Auto' CHECK(heater_status IN ('On','Off','Auto')),
                    ac_status TEXT DEFAULT 'Auto' CHECK(ac_status IN ('On','Off','Auto')),
                    light_status TEXT DEFAULT 'OFF' CHECK(light_status IN ('ON','OFF')),
                    fan_speed TEXT DEFAULT 'Medium' CHECK(fan_speed IN ('Low','Medium','High')),
                    vacation_start_date TEXT,
                    vacation_end_date TEXT,
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                );

                -- Copy all existing data
                INSERT INTO profiles_new (name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed, vacation_start_date, vacation_end_date, updated_at)
                SELECT 
                    name, 
                    mode, 
                    target_temp, 
                    greeting, 
                    description,
                    heater_status,
                    ac_status,
                    light_status,
                    'Medium' as fan_speed,
                    vacation_start_date,
                    vacation_end_date,
                    updated_at
                FROM profiles;

                -- Drop old table
                DROP TABLE profiles;

                -- Rename new table
                ALTER TABLE profiles_new RENAME TO profiles;
                "#
            )?;
        }
    }
    
    Ok(())
}

fn migrate_hvac_state_table(conn: &Connection) -> Result<()> {
    // Check if light_status column exists in hvac_state table
    let light_column_check: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('hvac_state') WHERE name='light_status'",
        [],
        |r| r.get(0),
    );
    
    if let Ok(count) = light_column_check {
        if count == 0 {
            // Recreate table with light_status column (no ALTER TABLE)
            conn.execute_batch(
                r#"
                -- Create new table with light_status column
                CREATE TABLE hvac_state_new (
                    id INTEGER PRIMARY KEY CHECK(id = 1),
                    mode TEXT NOT NULL CHECK(mode IN ('Off','Heating','Cooling','FanOnly','Auto')),
                    target_temperature REAL NOT NULL,
                    light_status TEXT DEFAULT 'OFF' CHECK(light_status IN ('ON','OFF')),
                    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
                );

                -- Copy existing data
                INSERT INTO hvac_state_new (id, mode, target_temperature, light_status, updated_at)
                SELECT 
                    id, 
                    mode, 
                    target_temperature,
                    'OFF' as light_status,
                    updated_at
                FROM hvac_state;

                -- Drop old table
                DROP TABLE hvac_state;

                -- Rename new table
                ALTER TABLE hvac_state_new RENAME TO hvac_state;
                "#
            )?;
        }
    }
    
    Ok(())
}

fn migrate_security_log_table(conn: &Connection) -> Result<()> {
    // Check if we need to migrate by examining the table schema
    let needs_migration = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='security_log'",
        [],
        |r| r.get::<_, String>(0),
    ).optional()?;
    
    if let Some(schema) = needs_migration {
        // Check if schema contains the new event types
        if !schema.contains("ACCESS_GRANTED") || !schema.contains("TECH_ACCESS") {
            // Recreate table with updated CHECK constraint
            conn.execute_batch(
                r#"
                -- Create new table with updated CHECK constraint
                CREATE TABLE security_log_new (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    actor_username TEXT NOT NULL,
                    target_username TEXT NOT NULL,
                    event_type TEXT NOT NULL CHECK(
                        event_type IN (
                            'ACCOUNT_CREATED', 'SUCCESS_LOGIN', 'FAILURE_LOGIN', 'LOGOUT', 
                            'LOCKOUT', 'SESSION_LOCKOUT', 'LOCKOUT_CLEARED',
                            'ACCOUNT_DELETED', 'ACCOUNT_DISABLED', 'ACCOUNT_ENABLED', 
                            'ADMIN_LOGIN', 'PASSWORD_CHANGE', 'HVAC',
                            'ACCESS_GRANTED', 'TECH_ACCESS', 'ACCESS_EXPIRED'
                        )
                    ),
                    description TEXT,
                    timestamp TEXT NOT NULL DEFAULT (datetime('now'))
                );

                -- Copy existing data
                INSERT INTO security_log_new (id, actor_username, target_username, event_type, description, timestamp)
                SELECT id, actor_username, target_username, event_type, description, timestamp
                FROM security_log;

                -- Drop old table
                DROP TABLE security_log;

                -- Rename new table
                ALTER TABLE security_log_new RENAME TO security_log;

                -- Recreate indexes
                CREATE INDEX ix_security_log_actor ON security_log(actor_username);
                CREATE INDEX ix_security_log_target ON security_log(target_username);
                "#
            )?;
        }
    }
    
    Ok(())
}

fn seed_default_profiles(conn: &Connection) -> Result<()> {
    // Insert if missing
    let defaults = ["Day", "Night", "Sleep", "Party", "Vacation", "Away"];
    for name in defaults.iter() {
        if let Some(def) = default_profile_row(name) {
            conn.execute(
                "INSERT OR IGNORE INTO profiles (name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![def.name, def.mode, def.target_temp, def.greeting, def.description, def.heater_status, def.ac_status, def.light_status, def.fan_speed],
            )?;
        }
    }
    Ok(())
}

pub fn get_profile_row(conn: &Connection, name: &str) -> Result<Option<ProfileRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed, vacation_start_date, vacation_end_date FROM profiles WHERE name = ?1",
    )?;
    let row = stmt
        .query_row(params![name], |r| {
            Ok(ProfileRow {
                name: r.get::<_, String>(0)?,
                mode: r.get::<_, String>(1)?,
                target_temp: r.get::<_, f32>(2)?,
                greeting: r.get::<_, Option<String>>(3)?,
                description: r.get::<_, Option<String>>(4)?,
                heater_status: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "Auto".to_string()),
                ac_status: r.get::<_, Option<String>>(6)?.unwrap_or_else(|| "Auto".to_string()),
                light_status: r.get::<_, Option<String>>(7)?.unwrap_or_else(|| "OFF".to_string()),
                fan_speed: r.get::<_, Option<String>>(8)?.unwrap_or_else(|| "Medium".to_string()),
                vacation_start_date: r.get::<_, Option<String>>(9)?,
                vacation_end_date: r.get::<_, Option<String>>(10)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn list_profile_rows(conn: &Connection) -> Result<Vec<ProfileRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed, vacation_start_date, vacation_end_date FROM profiles ORDER BY name",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProfileRow {
                name: r.get(0)?,
                mode: r.get(1)?,
                target_temp: r.get(2)?,
                greeting: r.get(3)?,
                description: r.get(4)?,
                heater_status: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "Auto".to_string()),
                ac_status: r.get::<_, Option<String>>(6)?.unwrap_or_else(|| "Auto".to_string()),
                light_status: r.get::<_, Option<String>>(7)?.unwrap_or_else(|| "OFF".to_string()),
                fan_speed: r.get::<_, Option<String>>(8)?.unwrap_or_else(|| "Medium".to_string()),
                vacation_start_date: r.get(9)?,
                vacation_end_date: r.get(10)?,
            })
        })?;
    let mut out = Vec::new();
    for r in rows { out.push(r?); }
    Ok(out)
}

#[allow(dead_code)]
pub fn update_profile_row(
    conn: &Connection,
    name: &str,
    mode: &str,
    target_temp: f32,
    greeting: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO profiles (name, mode, target_temp, greeting, description, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(name) DO UPDATE SET mode = excluded.mode, target_temp = excluded.target_temp, greeting = excluded.greeting, description = excluded.description, updated_at = datetime('now')",
        params![name, mode, target_temp, greeting, description],
    )?;
    Ok(())
}

pub fn reset_profile_to_default(conn: &Connection, name: &str) -> Result<()> {
    if let Some(def) = default_profile_row(name) {
        conn.execute(
            "INSERT INTO profiles (name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed, vacation_start_date, vacation_end_date, updated_at) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, datetime('now'))
             ON CONFLICT(name) DO UPDATE SET mode = excluded.mode, target_temp = excluded.target_temp, greeting = excluded.greeting, description = excluded.description, 
             heater_status = excluded.heater_status, ac_status = excluded.ac_status, light_status = excluded.light_status, fan_speed = excluded.fan_speed, vacation_start_date = NULL, vacation_end_date = NULL, updated_at = datetime('now')",
            params![def.name, def.mode, def.target_temp, def.greeting, def.description, def.heater_status, def.ac_status, def.light_status, def.fan_speed],
        )?;
    }
    Ok(())
}

// Set vacation dates for the Vacation profile
#[allow(dead_code)]
pub fn set_vacation_dates(conn: &Connection, start_date: &str, end_date: &str) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET vacation_start_date = ?1, vacation_end_date = ?2, updated_at = datetime('now') WHERE name = 'Vacation'",
        params![start_date, end_date],
    )?;
    Ok(())
}

// Clear vacation dates
#[allow(dead_code)]
pub fn clear_vacation_dates(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET vacation_start_date = NULL, vacation_end_date = NULL, updated_at = datetime('now') WHERE name = 'Vacation'",
        [],
    )?;
    Ok(())
}

// ======================================================
//          PROFILE MANAGEMENT (CREATE/DELETE)
// ======================================================

const DEFAULT_PROFILES: [&str; 6] = ["Day", "Night", "Sleep", "Party", "Vacation", "Away"];

// Check if a profile name is a default/protected profile
pub fn is_default_profile(name: &str) -> bool {
    DEFAULT_PROFILES.iter().any(|&p| p.eq_ignore_ascii_case(name))
}

// Validate profile name (3-20 chars, letters/numbers/spaces only, no duplicates)
pub fn validate_profile_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let trimmed = name.trim();
    
    // Check length
    if trimmed.len() < 3 || trimmed.len() > 20 {
        return Ok(Some("Profile name must be 3-20 characters long".to_string()));
    }
    
    // Check allowed characters (letters, numbers, spaces)
    if !trimmed.chars().all(|c| c.is_alphanumeric() || c.is_whitespace()) {
        return Ok(Some("Profile name can only contain letters, numbers, and spaces".to_string()));
    }
    
    // Check if it's a protected default profile name
    if is_default_profile(trimmed) {
        return Ok(Some("Cannot use default profile names (Day/Night/Sleep/Party/Vacation/Away)".to_string()));
    }
    
    // Check for duplicates (case-insensitive)
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM profiles WHERE LOWER(name) = LOWER(?1)",
        params![trimmed],
        |r| r.get(0),
    )?;
    
    if exists > 0 {
        return Ok(Some(format!("Profile '{}' already exists", trimmed)));
    }
    
    Ok(None) // No error
}

// Create a new custom profile
pub fn create_profile(
    conn: &Connection,
    name: &str,
    mode: &str,
    target_temp: f32,
    greeting: Option<&str>,
    description: Option<&str>,
    heater_status: &str,
    ac_status: &str,
    light_status: &str,
    fan_speed: &str,
) -> Result<()> {
    // Validate the profile name
    if let Some(error) = validate_profile_name(conn, name)? {
        return Err(anyhow!(error));
    }
    
    // Insert the new profile
    conn.execute(
        "INSERT INTO profiles (name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed, updated_at) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
        params![name.trim(), mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed],
    )?;
    
    Ok(())
}

// Delete a custom profile (cannot delete default profiles)
pub fn delete_profile(conn: &Connection, name: &str) -> Result<()> {
    // Check if it's a default profile
    if is_default_profile(name) {
        return Err(anyhow!("Cannot delete default profile '{}'", name));
    }
    
    // Check if profile exists
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM profiles WHERE name = ?1 COLLATE NOCASE",
        params![name],
        |r| r.get(0),
    )?;
    
    if exists == 0 {
        return Err(anyhow!("Profile '{}' does not exist", name));
    }
    
    // Delete the profile
    conn.execute(
        "DELETE FROM profiles WHERE name = ?1 COLLATE NOCASE",
        params![name],
    )?;
    
    Ok(())
}

// Update profile with full control over all parameters
pub fn update_profile_full(
    conn: &Connection,
    name: &str,
    mode: &str,
    target_temp: f32,
    greeting: Option<&str>,
    description: Option<&str>,
    heater_status: &str,
    ac_status: &str,
    light_status: &str,
    fan_speed: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET mode = ?2, target_temp = ?3, greeting = ?4, description = ?5, 
         heater_status = ?6, ac_status = ?7, light_status = ?8, fan_speed = ?9, updated_at = datetime('now') 
         WHERE name = ?1 COLLATE NOCASE",
        params![name, mode, target_temp, greeting, description, heater_status, ac_status, light_status, fan_speed],
    )?;
    
    Ok(())
}

// ======================================================
//              HVAC ACTIVITY LOGGING
// ======================================================

// Log when a user applies a profile
pub fn log_profile_applied(
    conn: &Connection,
    username: &str,
    user_role: &str,
    profile_name: &str,
    mode: &str,
    temperature: f32,
) -> Result<()> {
    let description = format!("📋 Profile applied: {} (⚙️ Mode: {}, 🌡️ Temp: {:.1}°C)", profile_name, mode, temperature);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, profile_name, new_value, description) 
         VALUES (?1, ?2, 'PROFILE_APPLIED', ?3, ?4, ?5)",
        params![username, user_role, profile_name, format!("{}|{:.1}", mode, temperature), description],
    )?;
    Ok(())
}

// Log when a homeowner/admin edits a profile
#[allow(dead_code)]
pub fn log_profile_edited(
    conn: &Connection,
    username: &str,
    user_role: &str,
    profile_name: &str,
    old_mode: Option<&str>,
    new_mode: &str,
    old_temp: Option<f32>,
    new_temp: f32,
) -> Result<()> {
    let old_value = old_mode.map(|m| format!("{}|{:.1}", m, old_temp.unwrap_or(0.0)));
    let new_value = format!("{}|{:.1}", new_mode, new_temp);
    let description = format!("✏️ Profile edited: {} | ⚙️ Mode: {} → {} | 🌡️ Temp: {:.1}°C → {:.1}°C", 
                             profile_name, 
                             old_mode.unwrap_or("unknown"), 
                             new_mode, 
                             old_temp.unwrap_or(0.0), 
                             new_temp);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, profile_name, old_value, new_value, description) 
         VALUES (?1, ?2, 'PROFILE_EDITED', ?3, ?4, ?5, ?6)",
        params![username, user_role, profile_name, old_value, new_value, description],
    )?;
    Ok(())
}

// Log when a profile is reset to defaults
pub fn log_profile_reset(
    conn: &Connection,
    username: &str,
    user_role: &str,
    profile_name: &str,
) -> Result<()> {
    let description = format!("🔄 Profile reset: {} restored to default settings", profile_name);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, profile_name, description) 
         VALUES (?1, ?2, 'PROFILE_RESET', ?3, ?4)",
        params![username, user_role, profile_name, description],
    )?;
    Ok(())
}

// Log when temperature is changed directly (not via profile)
pub fn log_temperature_changed(
    conn: &Connection,
    username: &str,
    user_role: &str,
    old_temp: f32,
    new_temp: f32,
) -> Result<()> {
    let description = format!("🌡️ Temperature changed: {:.1}°C → {:.1}°C", old_temp, new_temp);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, old_value, new_value, description) 
         VALUES (?1, ?2, 'TEMPERATURE_CHANGED', ?3, ?4, ?5)",
        params![username, user_role, format!("{:.1}", old_temp), format!("{:.1}", new_temp), description],
    )?;
    Ok(())
}

// Log when HVAC mode is changed directly (not via profile)
pub fn log_mode_changed(
    conn: &Connection,
    username: &str,
    user_role: &str,
    old_mode: &str,
    new_mode: &str,
) -> Result<()> {
    let description = format!("⚙️ Mode changed: {} → {}", old_mode, new_mode);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, old_value, new_value, description) 
         VALUES (?1, ?2, 'MODE_CHANGED', ?3, ?4, ?5)",
        params![username, user_role, old_mode, new_mode, description],
    )?;
    Ok(())
}

// View HVAC activity logs (for admins/homeowners)
#[allow(dead_code)]
pub fn view_hvac_activity_log(conn: &Connection, _username: &str, user_role: &str) -> Result<()> {
    // Only admins, homeowners, and technicians can view logs
    if user_role != "admin" && user_role != "homeowner" && user_role != "technician" {
        println!("Access denied: Only admins, homeowners, and technicians can view HVAC activity logs.");
        return Ok(());
    }

    let mut stmt = conn.prepare(
        "SELECT timestamp, username, user_role, action_type, profile_name, description 
         FROM hvac_activity_log 
         ORDER BY id DESC 
         LIMIT 50"
    )?;

    let logs = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,  // timestamp
            r.get::<_, String>(1)?,  // username
            r.get::<_, String>(2)?,  // user_role
            r.get::<_, String>(3)?,  // action_type
            r.get::<_, Option<String>>(4)?,  // profile_name
            r.get::<_, Option<String>>(5)?,  // description
        ))
    })?;

    println!("\n=== HVAC ACTIVITY LOG (Last 50 Entries) ===\n");

    let mut found_any = false;
    for log in logs {
        let (ts, user, role, action, profile, desc) = log?;
        found_any = true;
        
        // Convert UTC to EST for display
        let ts_display = to_eastern_time(&ts).unwrap_or(ts);
        let profile_str = profile.unwrap_or_else(|| "-".to_string());
        let desc_str = desc.unwrap_or_else(|| "".to_string());
        
        println!("─────────────────────────────────────────────────────────────────────");
        println!("Time: {} | User: {} ({}) | Action: {}", 
                 ts_display, user, role, action);
        if !profile_str.is_empty() && profile_str != "-" {
            println!("Profile: {}", profile_str);
        }
        if !desc_str.is_empty() {
            println!("Details: {}", desc_str);
        }
    }

    if !found_any {
        println!("(No HVAC activity logged yet.)");
    }

    println!("─────────────────────────────────────────────────────────────────────");
    Ok(())
}

pub fn insert_weather(conn: &mut Connection, data: &WeatherRecord) -> Result<()> {

    let tx = conn.transaction()?;
    {
        // Use parameterized query -> avoid SQL injection
        let mut stmt = tx.prepare_cached(
            "INSERT INTO weather (time, temperature_f, temperature_c, dewpoint_f, dewpoint_c, humidity, wind_speed_mph, wind_direction_deg, condition)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        )?;

        stmt.execute(params![
            &data.time,
            data.temperature_f,
            data.temperature_c,
            data.dewpoint_f,
            data.dewpoint_c,
            data.humidity,
            data.wind_speed_mph,
            data.wind_direction_deg,
            &data.condition,
        ])?;
    }
    tx.commit()?; // If excute fail, rollback
    Ok(())
}

// Get current HVAC state from database
pub fn get_hvac_state(conn: &Connection) -> Result<(String, f32, String)> {
    let mut stmt = conn.prepare("SELECT mode, target_temperature, light_status FROM hvac_state WHERE id = 1")?;
    let result = stmt.query_row([], |row| {
        Ok((
            row.get::<_, String>(0)?, 
            row.get::<_, f32>(1)?,
            row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "OFF".to_string())
        ))
    })?;
    Ok(result)
}

// Save current HVAC state to database
pub fn save_hvac_state(conn: &Connection, mode: &str, target_temperature: f32, light_status: &str) -> Result<()> {
    conn.execute(
        "UPDATE hvac_state SET mode = ?1, target_temperature = ?2, light_status = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
        params![mode, target_temperature, light_status],
    )?;
    Ok(())

}






