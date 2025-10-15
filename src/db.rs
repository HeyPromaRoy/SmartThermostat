use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

/// Initialize all required database tables and indexes.
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            username        TEXT NOT NULL UNIQUE,
            hashed_password TEXT NOT NULL,
            user_status     TEXT CHECK(user_status IN ('admin','homeowner','guest','technician')) NOT NULL,
            homeowner_id    INTEGER REFERENCES users(id),
            last_login_time TEXT,
            created_at      TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at      TEXT
        );

        CREATE INDEX IF NOT EXISTS ix_users_homeowner_id ON users(homeowner_id);
        "#,
    )?;
    Ok(())
}

/// Check if a username already exists.
pub fn user_exists(conn: &Connection, username: &str) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM users WHERE username = ?1",
            params![username],
            |r| r.get(0),
        )
        .context("Failed to query user existence")?;
    Ok(count > 0)
}

/// Retrieve a user's ID and role by username.
pub fn get_user_id_and_role(conn: &Connection, username: &str) -> Result<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, user_status FROM users WHERE username = ?1",
            params![username],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?)
}

/// Log a successful or failed login attempt to the audit table.
pub fn log_attempt(conn: &Connection, username: &str, success: bool) -> Result<()> {
    let status = if success { "SUCCESS" } else { "FAILURE" };
    let timestamp = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO login_log (username, status, timestamp)
         VALUES (?1, ?2, ?3)",
        params![username, status, timestamp],
    )
    .context("Failed to record login attempt")?;

    Ok(())
}

/// Insert a new user record (used internally by registration).
pub fn insert_user(
    conn: &mut Connection,
    username: &str,
    hashed: &str,
    role: &str,
    homeowner_id: Option<i64>,
) -> Result<()> {
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

/// Delete a guest account belonging to a specific homeowner.
pub fn delete_guest(conn: &Connection, homeowner_id: i64, guest_name: &str) -> Result<bool> {
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

/// List all guests registered under a given homeowner.
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
