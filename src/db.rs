use anyhow::{Context, Result};
use chrono::{DateTime, Utc, NaiveDateTime};
use chrono_tz::America::New_York;
use rusqlite::{params, Connection, OptionalExtension};
use rpassword::read_password;
use std::io::{self, Write};
use zeroize::Zeroize;

use crate::auth;
use crate::logger;


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
pub fn init_user_db(conn: &Connection) -> Result<()> {
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

    // create users table with 
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            username        TEXT NOT NULL UNIQUE,
            hashed_password TEXT NOT NULL,
            user_status     TEXT CHECK(user_status IN ('admin','homeowner','guest','technician')) NOT NULL,
            homeowner_id    INTEGER REFERENCES users(id),
            is_active       INTEGER DEFAULT 1,
            last_login_time TEXT,
            created_at      TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at      TEXT
        );

        CREATE INDEX IF NOT EXISTS ix_users_homeowner_id ON users(homeowner_id);
        CREATE INDEX IF NOT EXISTS ix_users_username ON users(username);
        "#,
    )
    .context("Failed to initialize users table")?;
    Ok(())
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
pub fn insert_user(
    conn: &mut Connection,
    username: &str,
    hashed: &str,
    role: &str,
    homeowner_id: Option<i64>) -> Result<()> {
    let tx = conn.transaction().context("Failed to start transaction")?;
    tx.execute(
        "INSERT INTO users (username, hashed_password, user_status, homeowner_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        params![username, hashed, role, homeowner_id],
    )
    .context("Failed to insert user")?;
    tx.commit().context("Failed to commit transaction")?;
    Ok(())
}

// Register a new guest account linked to a specific homeowner ID.
// This avoids username conflicts and ensures strong foreign key integrity.
pub fn add_guest(
    conn: &mut Connection,
    homeowner_id: i64,
    guest_username: &str,
    guest_hashed_pin: &str) -> Result<()> {
    // Verify that homeowner_id actually belongs to a homeowner
    let role: Option<String> = conn
        .query_row(
            "SELECT user_status FROM users WHERE id = ?1",
            params![homeowner_id],
            |r| r.get(0),
        )
        .optional()
        .context("Failed to verify homeowner ID")?;

    match role.as_deref() {
        Some("homeowner") => {
            insert_user(conn, guest_username, guest_hashed_pin, "guest", Some(homeowner_id))
                .context("Failed to add guest under homeowner")
        }
        Some(other) => anyhow::bail!("User ID {} is not a homeowner (role: {})", homeowner_id, other),
        None => anyhow::bail!("Homeowner ID {} not found", homeowner_id),
    }
}


// Delete a guest account belonging to a specific homeowner.
pub fn delete_guest(conn: &mut Connection, homeowner_id: i64, guest_name: &str) -> Result<bool> {
    let belongs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users
             WHERE username = ?1
             AND user_status = 'guest'
             AND homeowner_id = ?2",
            params![guest_name, homeowner_id],
            |r| r.get(0),
        )
        .context("Failed to verify guest ownership")?;

    if belongs == 0 {
        return Ok(false);
    }

    conn.execute(
        "DELETE FROM users
         WHERE username = ?1
         AND homeowner_id = ?2",
        params![guest_name, homeowner_id],
    )
    .context("Failed to delete guest")?;

    Ok(true)
}


// Log a successful or failed login attempt to the audit table.
pub fn log_attempt(conn: &Connection, username: &str, success: bool) -> Result<()> {
    let status = if success { "SUCCESS" } else { "FAILURE" };
    let timestamp = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO security_log (username, status, timestamp)
         VALUES (?1, ?2, ?3)",
        params![username, status, timestamp],
    )
    .context("Failed to record login attempt")?;

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
            })
            .context("Failed to retrieve guest rows")?;

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

    let logger_conn = logger::init_logger_db()?;

    // Verify admin identity
    println!("\nAdmin re-authentication required.");
    print!("Enter your password: ");
    io::stdout().flush().ok();

    let password = read_password().unwrap_or_default().trim().to_string();
    let stored_hash: Option<String> = conn
        .query_row(
            "SELECT hashed_password FROM users WHERE username = ?1 COLLATE NOCASE",
            params![admin_username],
            |r| r.get(0),
        )
        .optional()?;

    if stored_hash
        .as_ref()
        .map(|h| auth::verify_password(&password, h).unwrap_or(false))
        != Some(true)
    {
        println!("Authentication failed. Aborting.");
        return Ok(());
    }

    // Clear password memory
    let mut pw_clear = password;
    pw_clear.zeroize();

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

    logger::log_event(&logger_conn, admin_username, Some(&target_username), event_type, Some(&desc))?;
    Ok(())
}
