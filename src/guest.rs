use anyhow::{Result};
use rpassword::read_password; // hidden password entry for CLI
use std::io::{self, Write}; // reading inputs and printing prompts
use zeroize::Zeroizing; // used for sensitive data are wiped from the memory after use
use rusqlite::{params, Connection, OptionalExtension}; // handle for executing SQL queries

use crate::logger;
use crate::db;
use crate::auth;
use crate::ui;


// Guest login using PIN authentication
pub fn guest_login_user(conn: &mut Connection) -> Result<Option<String>> {
    // Single in-process session guard
   { 
    let active = auth::ACTIVE_SESSION
        .lock()
        .map_err(|_| anyhow::anyhow!("Failed to acquire ACTIVE_SESSION lock"))?;
    if let Some(ref current) = *active {
        println!("User '{current}' is already logged in. Please log out first.");
        return Ok(None);
        }
    }
    
    db::update_session(conn, None)?;

    if logger::session_lockout_check(conn, None)? {
        println!("Session temporarily locked due to repeated failed attempts.");
        return Ok(None);
    }

    // Prompt for username
    print!("Guest username: ");
    io::stdout().flush().ok();
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim().to_string();

    if username.is_empty() {
        println!("Username cannot be empty.");
        return Ok(None);
    }

    // Check lockout
    if logger::check_lockout(conn, &username)? {
        return Ok(None);
    }

    if logger::session_lockout_check(conn, Some(&username))? {
    println!("Session temporarily locked due to repeated failed attempts.");
    return Ok(None);
    }

    // Ask for guest PIN (hidden input)
    print!("Enter PIN: ");
    io::stdout().flush().ok();
    let pin_in = Zeroizing::new(read_password()?);
    let pin = pin_in.trim_end_matches(['\r', '\n']); // &str view; buffer wiped on drop

    // Fetch stored hash for this guest user
    let row = conn
        .query_row(
            "SELECT hashed_password, is_active
             FROM users WHERE username = ?1 AND user_status = 'guest' COLLATE NOCASE",
            params![username],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;

    let fake_hash = "$argon2id$v=19$m=65536,t=3,p=1$ABCdef123Q$hR2eWkj4jvIY6MfGfQ/fZg";
    
    let (stored_hash, is_active) = match row {
        Some(pair) => pair,
        None => {
            let _ = auth::verify_password(&pin, &fake_hash); // fake verify to normalize timing
            logger::fake_verification_delay();
            logger::record_login_attempt(conn, &username, false)?;
            logger::increment_session_fail(conn, None)?;
            logger::session_lockout_check(conn, None)?;
            println!("Invalid username or password.");
            return Ok(None);
        }
    };

    // Verify PIN
    if !auth::verify_password(&pin, &stored_hash)? {
        logger::fake_verification_delay();
        logger::record_login_attempt(conn, &username, false)?;
        logger::increment_session_fail(conn, Some(&username))?;
        logger::session_lockout_check(conn, Some(&username))?;
        println!("Invalid username or password.");
        return Ok(None);
    }

    // Disabled account
    if is_active == 0 {
        println!("This account has been disabled. Please contact your homeowner!");
        logger::record_login_attempt(conn, &username, false)?;
        return Ok(None);
    }

    // cleanup of expired sessions
    let _ = conn.execute(
        "DELETE FROM session_state WHERE session_expires <= datetime('now')",
        [],
    );

    // Deny concurrent login if a live session already exists
    let has_live_session: Option<i64> = conn
        .query_row(
            "SELECT 1
               FROM session_state
              WHERE username = ?1 COLLATE NOCASE
                AND session_expires > datetime('now')
              LIMIT 1",
            params![&username],
            |r| r.get(0),
        )
        .optional()?;

    if has_live_session.is_some() {
        println!("Login failed. Please try again.");
        let _ = logger::log_event(
            conn,
            &username,
            Some(&username),
            "SESSION_LOCKOUT",
            Some("Concurrent active session"),
        );
        return Ok(None);
    }

    // Success: record login + create new session (stores only hash; returns plaintext token)
    logger::record_login_attempt(conn, &username, true)?;
    let _session_token_plain = db::update_session(conn, Some(&username))?; // DO NOT persist

    // Reflect the session in-process so logout_user can find it (CLI guard)
    {
        let mut guard = auth::ACTIVE_SESSION
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to acquire ACTIVE_SESSION lock"))?;
        *guard = Some(username.clone());
    }

    println!("Welcome, {username}!");
    Ok(Some(username))
}

// Enables a guest account belonging to the homeowner
pub fn enable_guest(conn: &mut Connection, homeowner_id: i64, homeowner_username: &str) -> Result<()> {
    //Ensure homeowner is valid and active (sanity check)
    // This prevents enabling guests if the homeowner account itself was disabled
    let homeowner_active: i64 = conn.query_row(
        "SELECT is_active FROM users WHERE id = ?1 AND user_status = 'homeowner'",
        params![homeowner_id],
        |r| r.get(0),
    )?;
    if homeowner_active == 0 {
        println!("Your account is inactive. Please contact an administrator.");
        return Ok(());
    }

    // List all guests owned by this homeowner
    let mut stmt = conn.prepare(
        "SELECT username, is_active, created_at, last_login_time
         FROM users
         WHERE user_status = 'guest'
           AND homeowner_id = ?1
         ORDER BY created_at DESC",
    )?;
    // execute the query and map each result row into a tuple
    let guests = stmt
        .query_map(params![homeowner_id], |r| {
            Ok((
                r.get::<_, String>(0)?, // guest username
                r.get::<_, i64>(1)?,    // active flag
                r.get::<_, String>(2)?, // created_at
                r.get::<_, Option<String>>(3)?, // optional last_login_time
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;


    if guests.is_empty() { //no existing guests
        println!("You have no registered guests.");
        return Ok(());
    }
    // Display guests for selection
    println!("\n Guests under your account:");
    for (i, (username, active, created_at, last_login)) in guests.iter().enumerate() {
        // convert the numeric active flag into readable status
        let status = if *active == 1 { "Active" } else { "Disabled" };
        println!( //show each guest with formatted details
            "{}. {} ({}) - Created: {} | Last Login: {}",
            i + 1,
            username,
            status,
            created_at,
            last_login.clone().unwrap_or_else(|| "never".into())
        );
    }

    //Prompt user to pick guest number
    print!("\nEnter the number of the guest to enable: ");
    io::stdout().flush().ok(); //prompt
    let mut choice = String::new();
    io::stdin().read_line(&mut choice).ok(); //read user input
    let choice = choice.trim().parse::<usize>();
    //extract guest from list by index
    let (guest_username, active) = match choice.ok().and_then(|n| guests.get(n - 1)) {
        // if valid, get username and active flag        
        Some((uname, active, _, _)) => (uname.clone(), *active),
        None => { //handle invalid input
            println!("Invalid selection.");
            return Ok(());
        }
    };
    // preventing redundant activation
    if active == 1 {
        println!("Guest '{}' is already active.", guest_username);
        return Ok(());
    }

    // Enable guest confirmation to prevent accidental actions
    print!("Confirm enabling guest '{}'? (yes/no): ", guest_username);
    io::stdout().flush().ok();
    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm).ok();
    if confirm.trim().to_lowercase() != "yes" { //if user didn't type yes, cancel
        println!("Action cancelled.");
        return Ok(());
    }

    //Execute DB update in transcation
    drop(stmt);
    let tx = conn.transaction()?;
    let affected = tx.execute(
        "UPDATE users
         SET is_active = 1, updated_at = datetime('now')
         WHERE username = ?1
           AND homeowner_id = ?2",
        params![guest_username, homeowner_id],
    )?;
    tx.commit()?; //commit if successfuly

    if affected > 0 { //provide feedback
        println!("Guest '{}' has been enabled successfully.", guest_username);
        logger::log_event(conn, homeowner_username, Some(&guest_username), "ACCOUNT_ENABLED", Some("Homeowner enabled guest account"))?;
    } else {
        println!("Failed to enable guest '{}'.", guest_username);
    }

    Ok(())
}

// Disables a guest account owned by the authenticated homeowner.
/// Uses `homeowner_id` for secure identity binding instead of relying on usernames.
pub fn disable_guest(conn: &mut Connection, homeowner_id: i64, homeowner_username: &str) -> Result<()> {
    //Ensure homeowner is valid and active (sanity check)
    // This prevents enabling guests if the homeowner account itself was disabled
    let homeowner_active: i64 = conn.query_row(
        "SELECT is_active FROM users WHERE id = ?1 AND user_status = 'homeowner'",
        params![homeowner_id],
        |r| r.get(0),
    )?;
    if homeowner_active == 0 {
        println!("Your account is inactive. Please contact an administrator.");
        return Ok(());
    }

    // List all guests owned by this homeowner
    let mut stmt = conn.prepare(
        "SELECT username, is_active, created_at, last_login_time
         FROM users WHERE user_status = 'guest' AND homeowner_id = ?1
         ORDER BY created_at DESC",
    )?;

    
    let guests = stmt
        .query_map(params![homeowner_id], |r| {
            Ok((
                r.get::<_, String>(0)?, // username
                r.get::<_, i64>(1)?,    // is_active
                r.get::<_, String>(2)?, // created_at
                r.get::<_, Option<String>>(3)?, // last_login_time
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

        drop(stmt);

    if guests.is_empty() { //no existing guest users
        println!("You have no registered guests.");
        return Ok(());
    }

    // Display guests for selection
    println!("\nGuests under your account:");
    for (i, (username, active, created_at, last_login)) in guests.iter().enumerate() {
        let status = if *active == 1 { "Active" } else { "Disabled" };
        println!(
            "{}. {} ({}) - Created: {} | Last Login: {}",
            i + 1,
            username,
            status,
            created_at,
            last_login.clone().unwrap_or_else(|| "never".into())
        );
    }

    // Prompt user to pick one
    print!("\nEnter the number of the guest to disable: ");
    io::stdout().flush().ok();
    let mut choice = String::new();
    io::stdin().read_line(&mut choice).ok();
    let choice = choice.trim().parse::<usize>();

    let (guest_username, active) = match choice.ok().and_then(|n| guests.get(n - 1)) {
        Some((uname, active, _, _)) => (uname.clone(), *active),
        None => {
            println!("Invalid selection.");
            return Ok(());
        }
    };
    // Prevent re-disabling
    if active == 0 {
        println!("Guest '{}' is already disabled.", guest_username);
        return Ok(());
    }

    // Verify homeowner password before modifying guest status
    println!("\nPlease verify your identity to disable '{}':", guest_username);
    print!("Enter your password: ");
    io::stdout().flush().ok();
    let pw_in = Zeroizing::new(read_password()?);
    let password = pw_in.trim_end_matches(['\r', '\n']);

    // Fetch stored hash for the homeowner
    let stored_hash_opt: Option<String> = conn
        .query_row(
            "SELECT hashed_password FROM users 
             WHERE username = ?1 AND user_status = 'homeowner' COLLATE NOCASE",
            params![homeowner_username],
            |r| r.get(0),
        )
        .optional()?;

    // Verify password securely
    let auth_success = match stored_hash_opt {
        Some(stored_hash) => crate::auth::verify_password(&password, &stored_hash)?,
        None => {
            logger::fake_verification_delay();
            false
        }
    };

    // Authentication check
    if !auth_success {
    println!("Authentication failed. Action canceled.");
    return Ok(());
    }

    //only runs if authentication succeeded
    let affected = conn.execute(
    "UPDATE users
     SET is_active = 0, updated_at = datetime('now')
     WHERE username = ?1 AND homeowner_id = ?2",
    params![guest_username, homeowner_id],
    )?;

    // handle result
    if affected > 0 {
    println!("Guest '{}' has been disabled successfully.", guest_username);
    logger::log_event(conn, homeowner_username, Some(&guest_username), "ACCOUNT_ENABLED", Some("Homeowner disabled guest account"))?;
    } else {
    println!("Failed to disable guest '{}'.", guest_username);
    }

    Ok(())
}


// delete guest (ensures the guest belongs to homeowner)
pub fn delete_guest(conn: &mut Connection, homeowner_id: i64, homeowner_username: &str) -> Result<()> {
    //List all guests for this homeowner
    let mut stmt = conn.prepare(
        "SELECT username, is_active, created_at, last_login_time
         FROM users
         WHERE user_status = 'guest'
           AND homeowner_id = ?1
         ORDER BY created_at DESC",
    )?;

    let guests = stmt
        .query_map(params![homeowner_id], |r| {
            Ok((
                r.get::<_, String>(0)?,          // username
                r.get::<_, i64>(1)?,             // is_active
                r.get::<_, String>(2)?,          // created_at
                r.get::<_, Option<String>>(3)?,  // last_login_time
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if guests.is_empty() {
        println!("You have no registered guests.");
        return Ok(());
    }

    // Display all guests in a user-friendly format
    println!("\nGuests under your account:");
    for (i, (username, active, created_at, last_login)) in guests.iter().enumerate() {
        let status = if *active == 1 { "Active" } else { "Disabled" };
        println!(
            "{}. {} ({}) - Created: {} | Last Login: {}",
            i + 1,
            username,
            status,
            created_at,
            last_login.clone().unwrap_or_else(|| "never".into())
        );
    }

    //Ask user which guest to delete
    print!("\nEnter the number of the guest to delete: ");
    io::stdout().flush().ok();
    let mut choice = String::new();
    io::stdin().read_line(&mut choice).ok();
    let choice = choice.trim().parse::<usize>().ok();

    let (guest_username, _active) = match choice.and_then(|n| guests.get(n - 1)) {
        Some((uname, active, _, _)) => (uname.clone(), *active),
        None => {
            println!("Invalid selection.");
            return Ok(());
        }
    };

    //Verify homeowner identity via password
    println!("\nPlease verify your identity to delete '{}':", guest_username);
    print!("Enter your password: ");
    io::stdout().flush().ok();
    let pw_in = Zeroizing::new(read_password()?);
    let password = pw_in.trim_end_matches(['\r', '\n']);

    // Fetch the homeowner’s Argon2 hash
    let stored_hash_opt: Option<String> = conn
        .query_row(
            "SELECT hashed_password
             FROM users
             WHERE id = ?1 AND user_status = 'homeowner'",
            params![homeowner_id],
            |r| r.get(0),
        )
        .optional()?;

    // Verify password securely
    let auth_success = match stored_hash_opt {
        Some(stored_hash) => crate::auth::verify_password(&password, &stored_hash)?,
        None => {
            logger::fake_verification_delay();
            false
        }
    };


    if !auth_success {
        println!("Authentication failed. Action canceled.");
        return Ok(());
    }

    //  Delete guest safely in a transaction
    drop(stmt);
    let tx = conn.transaction()?;
    let affected = tx.execute(
        "DELETE FROM users
         WHERE username = ?1
           AND homeowner_id = ?2
           AND user_status = 'guest'",
        params![guest_username, homeowner_id],
    )?;
    tx.commit()?;

    // Report and log result
    if affected > 0 {
        println!("Guest '{}' has been deleted successfully.", guest_username);
        logger::log_event(conn, homeowner_username, Some(&guest_username), "ACCOUNT_DELETED", Some("Homeowner dleted guest account"))?;
    } else {
        println!("Failed to delete guest '{}'.", guest_username);
    }

    Ok(())
}


pub fn reset_guest_pin(conn: &mut Connection, homeowner_id: i64, homeowner_username: &str) -> Result<()> {
    //List all guests owned by this homeowner
    let mut stmt = conn.prepare(
        "SELECT username, is_active, created_at, last_login_time
         FROM users
         WHERE homeowner_id = ?1
           AND user_status = 'guest'
         ORDER BY created_at DESC"
    )?;

    let guests = stmt
        .query_map(params![homeowner_id], |r| {
            Ok((
                r.get::<_, String>(0)?,          // username
                r.get::<_, i64>(1)?,             // is_active
                r.get::<_, String>(2)?,          // created_at
                r.get::<_, Option<String>>(3)?,  // last_login_time
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if guests.is_empty() {
        println!("You have no registered guests.");
        return Ok(());
    }

    // Display guests neatly
    println!("\nGuests under your account:");
    for (i, (username, active, created_at, last_login)) in guests.iter().enumerate() {
        let status = if *active == 1 { "Active" } else { "Disabled" };
        println!(
            "{}. {} ({}) - Created: {} | Last Login: {}",
            i + 1,
            username,
            status,
            created_at,
            last_login.clone().unwrap_or_else(|| "never".into())
        );
    }

    // Let homeowner choose which guest’s PIN to reset
    print!("\nEnter the number of the guest to reset PIN: ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice = input.trim().parse::<usize>().ok();

    let (guest_username, active) = match choice.and_then(|n| guests.get(n - 1)) {
        Some((uname, active, _, _)) => (uname.clone(), *active),
        None => {
            println!("Invalid selection.");
            return Ok(());
        }
    };

    if active == 0 {
        println!("Guest '{}' is disabled. Please enable the guest first.", guest_username);
        return Ok(());
    }

    // Authenticate homeowner by password
    println!("\nPlease verify your identity before resetting '{}':", guest_username);
    print!("Enter your password: ");
    io::stdout().flush().ok();
    let pw_in = Zeroizing::new(read_password()?);
    let password = pw_in.trim_end_matches(['\r', '\n']);

    let stored_hash_opt: Option<String> = conn
        .query_row(
            "SELECT hashed_password FROM users WHERE id = ?1 AND user_status = 'homeowner'",
            params![homeowner_id],
            |r| r.get(0),
        )
        .optional()?;

    let auth_success = match stored_hash_opt {
        Some(stored_hash) => crate::auth::verify_password(&password, &stored_hash)?,
        None => {
            logger::fake_verification_delay();
            false
        }
    };

    if !auth_success {
        println!("Authentication failed. Action canceled.");
        return Ok(());
    }

    //Prompt for new PIN (min 6 chars)
    print!("\nEnter new PIN for '{}': ", guest_username);
    io::stdout().flush().ok();
    let new_pin_in: Zeroizing<String> = Zeroizing::new(read_password()?);
    let new_pin_trimmed: &str = new_pin_in.trim_end_matches(['\r','\n']);

    if new_pin_trimmed.len() < 6 {
        println!("PIN must be at least 6 characters long.");
        return Ok(());
    }

    //Hash new PIN securely (Argon2id)
    let hashed_pin = crate::auth::hash_password(&new_pin_trimmed)?;


    // Update guest’s PIN atomically
   drop(stmt);
    let tx = conn.transaction()?;
    let affected = tx.execute(
        "UPDATE users
         SET hashed_password = ?1,
             updated_at = datetime('now')
         WHERE username = ?2
           AND homeowner_id = ?3
           AND user_status = 'guest'",
        params![hashed_pin, guest_username, homeowner_id],
    )?;
    tx.commit()?;

    // Confirm 
    if affected > 0 {
        println!("PIN for '{}' has been successfully reset!", guest_username);
        logger::log_event(conn, homeowner_username, Some(&guest_username), "PASSWORD_CHANGE", None)?;
    } else {
        println!("Failed to reset PIN for '{}'.", guest_username);
    }

    Ok(())
}


pub fn manage_guests_menu(conn: &mut Connection, _owner_id: i64, 
    acting_username: &str, acting_role: &str, homeowner_username: &str) -> Result<()> {
    
     // Resolve homeowner
    let homeowner_id = match db::get_user_id_and_role(conn, homeowner_username)? {
        Some((id, role)) if role == "homeowner" => id,
        _ => {
            println!("Invalid homeowner '{}'.", homeowner_username);
            return Ok(());
        }
    };

    // Technician step-up auth
    if acting_role == "technician" {
        if !db::tech_has_perm(conn, acting_username, homeowner_username)? {
            println!("Access denied: no active job grant for '{}'.", homeowner_username);
            return Ok(());
        }

        println!("\nSecurity check for technician '{}'", acting_username);
        print!("Enter your technician password: ");
        io::stdout().flush().ok();

        let pw_in: Zeroizing<String> = Zeroizing::new(read_password()?);
        let pw_trimmed: &str = pw_in.trim_end_matches(['\r', '\n']);

        let stored_hash_opt: Option<String> = conn
            .query_row(
                "SELECT hashed_password FROM users
                 WHERE username = ?1 COLLATE NOCASE AND user_status = 'technician'",
                params![acting_username],
                |r| r.get(0),
            )
            .optional()?;

        let auth_ok = stored_hash_opt
            .as_deref()
            .map_or(false, |h| crate::auth::verify_password(pw_trimmed, h).unwrap_or(false));
        if !auth_ok {
            println!("Authentication failed. Returning.");
            return Ok(());
        }
    }

    // ---- Main loop ----
    loop {
        if acting_role == "technician"
            && !db::tech_has_perm(conn, acting_username, homeowner_username)?
        {
            println!("Grant expired or revoked for '{}'.", homeowner_username);
            break;
        }

        ui::manage_guest_menu();
        
        let mut choice = String::new();
        let n = std::io::stdin().read_line(&mut choice)?;
        if n == 0 {
            println!("Input closed. Returning to Menu...");
            break;
        }

        match choice.trim() {
            "1" => {
                println!("\n======= Reset Guest PIN =======");
                if let Err(e) = reset_guest_pin(conn, homeowner_id, homeowner_username) {
                    println!("Error: {}", e);
                }
            }
            "2" => {
                println!("\n======= Enable/Disable Guest =======");
                println!("[1] Enable Guest");
                println!("[2] Disable Guest");
                print!("Select an option [1-2]: ");
                io::stdout().flush().ok();

                let mut sub_choice = String::new();
                let m = std::io::stdin().read_line(&mut sub_choice).unwrap_or(0);
                if m == 0 {
                    println!("Input closed. Returning...");
                } else {
                    match sub_choice.trim() {
                        "1" => {
                            if let Err(e) = enable_guest(conn, homeowner_id, homeowner_username) {
                                println!("Error: {}", e);
                            }
                        }
                        "2" => {
                            if let Err(e) = disable_guest(conn, homeowner_id, homeowner_username) {
                                println!("Error: {}", e);
                            }
                        }
                        _ => println!("Invalid sub-option."),
                    }
                }
            }
            "3" => {
                println!("\n======= Delete Guest =======");
                if let Err(e) = delete_guest(conn, homeowner_id, homeowner_username) {
                    println!("Error: {}", e);
                }
            }
            "4" => {
                println!("Returning to Menu...");
                break;
            }
            _ => println!("Invalid choice, please enter 1–4."),
        }

        print!("\nPress ENTER to continue...");
        io::stdout().flush().ok();
        let mut dummy = String::new();
        let _ = std::io::stdin().read_line(&mut dummy);
        println!();
    }

    Ok(())
}
