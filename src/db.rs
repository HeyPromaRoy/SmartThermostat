use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose};
use blake3;
use chrono::{DateTime, Utc, NaiveDateTime};
use chrono_tz::America::New_York;
use rusqlite::{params, Connection, OptionalExtension};
use rpassword::read_password;
use rand::{RngCore, rngs::OsRng};
use std::io::{self, Write};
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
pub fn init_system_db() -> Result<Connection> {
    let conn = Connection::open("system.db").context("Failed to open system.db")?;
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
                    'ACCOUNT_DELETED', 'ACCOUNT_DISABLED', 'ACCOUNT_ENABLED', 'ADMIN_LOGIN', 'PASSWORD_CHANGE', 'HVAC'
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
            username TEXT NOT NULL UNIQUE COLLATE NOCASE,
            session_token_hash TEXT UNIQUE,
            login_time TEXT DEFAULT CURRENT_TIMESTAMP,
            last_active_time TEXT,
            session_expires TEXT,
            FOREIGN KEY(username) REFERENCES users(username) ON DELETE CASCADE ON UPDATE CASCADE
        );

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
        "#,
    )
    .context("Failed to initialize tables in system.db")?;

    // Seed default profiles if missing
    seed_default_profiles(&conn)?;

    Ok(conn)
}

// Returns a reusable SQLite connection to the unified database.
pub fn get_connection() -> Result<Connection> {
    init_system_db()
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


/// View all registered users (admin-only)
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
//                          TOKEN
// ======================================================

fn new_session_token() -> (Zeroizing<String>, String) {
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);

    // Plain token returned to caller (for headers/cookies/in-memory)
    let token_plain = Zeroizing::new(general_purpose::URL_SAFE_NO_PAD.encode(buf));

    // Hash stored in DB (hex)
    let token_hash_hex = blake3::hash(&buf).to_hex().to_string();
    (token_plain, token_hash_hex)
}

/*Creates or updates a user session in the `session_state` table.
    - Marks old sessions as inactive
    - Inserts a new session record with expiry 30 min from now
    - Returns the session_token for in-memory tracking */
pub fn update_session(conn: &Connection, username: &str) -> Result<String> {
    conn.execute(
        "DELETE FROM session_state WHERE username = ?1",
        params![username],
    )?;

    let (token_plain, token_hash_hex) = new_session_token();

    let expires = Utc::now() + chrono::Duration::minutes(10);
    let expires_str = expires.format("%Y-%m-%d %H:%M:%S").to_string();

    conn.execute(
        "INSERT INTO session_state (username, session_token_hash, login_time, session_expires)
         VALUES (?1, ?2, datetime('now'), ?3)",
        params![username, token_hash_hex, expires_str],
    )?;

    // Give the caller the plaintext token (not stored in DB)
    Ok(token_plain.to_string())}

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
}

fn default_profile_row(name: &str) -> Option<ProfileRow> {
    match name {
        "Day" => Some(ProfileRow {
            name: "Day".to_string(),
            mode: "Auto".to_string(),
            target_temp: 22.0,
            greeting: Some("☀️ Hope you have a good day!".to_string()),
            description: Some("Auto mode, comfort-oriented, 21-23°C / 24-26°C, Auto fan, Comfort".to_string()),
        }),
        "Night" => Some(ProfileRow {
            name: "Night".to_string(),
            mode: "Auto".to_string(),
            target_temp: 20.0,
            greeting: Some("🌙 Have a Good Night!".to_string()),
            description: Some("Auto or steady heating/cooling, 20°C heating / 25°C cooling, Low fan speed, Moderate".to_string()),
        }),
        "Sleep" => Some(ProfileRow {
            name: "Sleep".to_string(),
            mode: "Heating".to_string(),
            target_temp: 18.0,
            greeting: Some("😴 Sleep well and sweet dreams!".to_string()),
            description: Some("Heating preferred, quiet fan, 18-20°C heating / 26-28°C cooling, Fan off/low, Energy saving".to_string()),
        }),
        "Party" => Some(ProfileRow {
            name: "Party".to_string(),
            mode: "Cooling".to_string(),
            target_temp: 23.0,
            greeting: Some("🎊 Let's get this party started!".to_string()),
            description: Some("Cooling with ventilation, 22°C heating / 23-24°C cooling, Medium-high fan, Comfort prioritized".to_string()),
        }),
        "Vacation" => Some(ProfileRow {
            name: "Vacation".to_string(),
            mode: "Off".to_string(),
            target_temp: 24.0,
            greeting: Some("🏖️ Enjoy your vacation!".to_string()),
            description: Some("HVAC mostly off, 16-18°C heating / 29-30°C cooling, Fan off, Max energy saving".to_string()),
        }),
        "Away" => Some(ProfileRow {
            name: "Away".to_string(),
            mode: "Off".to_string(),
            target_temp: 25.0,
            greeting: Some("🚗 Have a safe trip!".to_string()),
            description: Some("HVAC off/eco mode, 17-18°C heating / 28°C cooling, Fan off, Energy saving".to_string()),
        }),
        _ => None,
    }
}

fn seed_default_profiles(conn: &Connection) -> Result<()> {
    // Insert if missing
    let defaults = ["Day", "Night", "Sleep", "Party", "Vacation", "Away"];
    for name in defaults.iter() {
        if let Some(def) = default_profile_row(name) {
            conn.execute(
                "INSERT OR IGNORE INTO profiles (name, mode, target_temp, greeting, description) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![def.name, def.mode, def.target_temp, def.greeting, def.description],
            )?;
        }
    }
    Ok(())
}

pub fn get_profile_row(conn: &Connection, name: &str) -> Result<Option<ProfileRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, mode, target_temp, greeting, description FROM profiles WHERE name = ?1",
    )?;
    let row = stmt
        .query_row(params![name], |r| {
            Ok(ProfileRow {
                name: r.get::<_, String>(0)?,
                mode: r.get::<_, String>(1)?,
                target_temp: r.get::<_, f32>(2)?,
                greeting: r.get::<_, Option<String>>(3)?,
                description: r.get::<_, Option<String>>(4)?,
            })
        })
        .optional()?;
    Ok(row)
}

pub fn list_profile_rows(conn: &Connection) -> Result<Vec<ProfileRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, mode, target_temp, greeting, description FROM profiles ORDER BY name",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProfileRow {
                name: r.get(0)?,
                mode: r.get(1)?,
                target_temp: r.get(2)?,
                greeting: r.get(3)?,
                description: r.get(4)?,
            })
        })?;
    let mut out = Vec::new();
    for r in rows { out.push(r?); }
    Ok(out)
}

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
            "INSERT INTO profiles (name, mode, target_temp, greeting, description, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
             ON CONFLICT(name) DO UPDATE SET mode = excluded.mode, target_temp = excluded.target_temp, greeting = excluded.greeting, description = excluded.description, updated_at = datetime('now')",
            params![def.name, def.mode, def.target_temp, def.greeting, def.description],
        )?;
    }
    Ok(())
}

// ======================================================
//              HVAC ACTIVITY LOGGING
// ======================================================

/// Log when a user applies a profile
pub fn log_profile_applied(
    conn: &Connection,
    username: &str,
    user_role: &str,
    profile_name: &str,
    mode: &str,
    temperature: f32,
) -> Result<()> {
    let description = format!("Applied {} profile: mode={}, temp={:.1}°C", profile_name, mode, temperature);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, profile_name, new_value, description) 
         VALUES (?1, ?2, 'PROFILE_APPLIED', ?3, ?4, ?5)",
        params![username, user_role, profile_name, format!("{}|{:.1}", mode, temperature), description],
    )?;
    Ok(())
}

/// Log when a homeowner/admin edits a profile
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
    let description = format!("Edited {} profile: mode {} -> {}, temp {:.1} -> {:.1}°C", 
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

/// Log when a profile is reset to defaults
pub fn log_profile_reset(
    conn: &Connection,
    username: &str,
    user_role: &str,
    profile_name: &str,
) -> Result<()> {
    let description = format!("Reset {} profile to default settings", profile_name);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, profile_name, description) 
         VALUES (?1, ?2, 'PROFILE_RESET', ?3, ?4)",
        params![username, user_role, profile_name, description],
    )?;
    Ok(())
}

/// Log when temperature is changed directly (not via profile)
pub fn log_temperature_changed(
    conn: &Connection,
    username: &str,
    user_role: &str,
    old_temp: f32,
    new_temp: f32,
) -> Result<()> {
    let description = format!("Changed temperature from {:.1}°C to {:.1}°C", old_temp, new_temp);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, old_value, new_value, description) 
         VALUES (?1, ?2, 'TEMPERATURE_CHANGED', ?3, ?4, ?5)",
        params![username, user_role, format!("{:.1}", old_temp), format!("{:.1}", new_temp), description],
    )?;
    Ok(())
}

/// Log when HVAC mode is changed directly (not via profile)
pub fn log_mode_changed(
    conn: &Connection,
    username: &str,
    user_role: &str,
    old_mode: &str,
    new_mode: &str,
) -> Result<()> {
    let description = format!("Changed mode from {} to {}", old_mode, new_mode);
    conn.execute(
        "INSERT INTO hvac_activity_log (username, user_role, action_type, old_value, new_value, description) 
         VALUES (?1, ?2, 'MODE_CHANGED', ?3, ?4, ?5)",
        params![username, user_role, old_mode, new_mode, description],
    )?;
    Ok(())
}

/// View HVAC activity logs (for admins/homeowners)
pub fn view_hvac_activity_log(conn: &Connection, username: &str, user_role: &str) -> Result<()> {
    // Only admins and homeowners can view logs
    if user_role != "admin" && user_role != "homeowner" {
        println!("Access denied: Only admins and homeowners can view HVAC activity logs.");
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

    println!("\n===== HVAC Activity Log (Last 50 entries) =====");
    println!("{:<20} {:<15} {:<12} {:<20} {}", "Timestamp", "User", "Role", "Action", "Description");
    println!("{}", "-".repeat(100));

    let mut found_any = false;
    for log in logs {
        let (ts, user, role, action, profile, desc) = log?;
        found_any = true;
        
        // Convert UTC to EST for display
        let ts_display = to_eastern_time(&ts).unwrap_or(ts);
        let profile_str = profile.unwrap_or_else(|| "-".to_string());
        let desc_str = desc.unwrap_or_else(|| "".to_string());
        
        println!("{:<20} {:<15} {:<12} {:<20} {}", 
                 ts_display, user, role, 
                 format!("{}:{}", action, profile_str), 
                 desc_str);
    }

    if !found_any {
        println!("(No HVAC activity logged yet.)");
    }

    println!("{}", "-".repeat(100));
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