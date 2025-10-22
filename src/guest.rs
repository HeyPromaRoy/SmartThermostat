use anyhow::{Context, Result};
use rpassword::read_password; // hidden password entry for CLI
use std::io::{self, Write}; // reading inputs and printing prompts
use zeroize::Zeroize; // used for sensitive data are wiped from the memory after use
use rusqlite::{params, Connection, OptionalExtension}; // handle for executing SQL queries

use crate::logger;
use crate::db;
use crate::auth;
use crate::ui;


// Guest login using PIN authentication

pub fn guest_login_user(conn: &mut Connection) -> Result<Option<String>> {
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

    // Ask for guest PIN (hidden input)
    print!("Enter PIN: ");
    io::stdout().flush().ok();
    let pin = read_password().context("Failed to read PIN")?;
    let pin = pin.trim().to_string();

    // Fetch stored hash for this guest user
    let stored_hash_opt = conn
        .query_row(
            "SELECT hashed_password, is_active
             FROM users WHERE username = ?1 AND user_status = 'guest' COLLATE NOCASE",
            params![username],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
        )
        .optional()?;

    let result = match stored_hash_opt {
        Some((stored_hash, is_active)) => {
            if is_active == 0 {
                println!("This account has been disabled. Please contact your homeowner!");
                logger::record_login_attempt(conn, &username, false)?;
                Ok(None)
            }
            else if auth::verify_password(&pin, &stored_hash)? {
                logger::fake_verification_delay();
                logger::record_login_attempt(conn, &username, true)?;

                // Update timestamps after successful login
                conn.execute(
                    "UPDATE users 
                     SET last_login_time = datetime('now'), 
                         updated_at = datetime('now') 
                     WHERE username = ?1 COLLATE NOCASE",
                    params![username],
                )?;

                println!("Welcome, {username}!");
                let _session_token = db::update_session(conn, &username)?;
                Ok(Some(username))
            } else {
                logger::fake_verification_delay();
                logger::record_login_attempt(conn, &username, false)?;
                println!("Invalid username or password.");
                Ok(None)
            }
        }
        None => {
            logger::fake_verification_delay();
            logger::record_login_attempt(conn, &username, false)?;
            println!("Invalid username or password.");
            Ok(None)
        }
    };

    // Securely zeroize PIN
    let mut clear_pin = pin;
    clear_pin.zeroize();

    result
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
         FROM users
         WHERE user_status = 'guest'
           AND homeowner_id = ?1
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
    let password = read_password().context("Failed to read password")?;
    let password = password.trim().to_string();

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

    // Immediately zeroize password from memory
    let mut clear_pw = password;
    clear_pw.zeroize();

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
    let password = read_password().context("Failed to read password")?;
    let password = password.trim().to_string();

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

    // Zeroize password from memory
    let mut clear_pw = password;
    clear_pw.zeroize();

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
    let password = read_password().context("Failed to read password")?;
    let password = password.trim().to_string();

    let stored_hash_opt: Option<String> = conn
        .query_row(
            "SELECT hashed_password
             FROM users
             WHERE id = ?1 AND user_status = 'homeowner'",
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

    let mut clear_pw = password;
    clear_pw.zeroize();

    if !auth_success {
        println!("Authentication failed. Action canceled.");
        return Ok(());
    }

    //Prompt for new PIN (min 6 chars)
    print!("\nEnter new PIN for '{}': ", guest_username);
    io::stdout().flush().ok();
    let mut new_pin = String::new();
    io::stdin().read_line(&mut new_pin)?;
    let new_pin = new_pin.trim().to_string();

    if new_pin.len() < 6 {
        println!("PIN must be at least 6 characters long.");
        return Ok(());
    }

    //Hash new PIN securely (Argon2id)
    let hashed_pin = crate::auth::hash_password(&new_pin)?;
    let mut pin_to_zeroize = new_pin;
    pin_to_zeroize.zeroize();

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


pub fn manage_guests_menu(conn: &mut Connection, owner_id: i64, homeowner_username: &str) -> Result<()> {
    loop {
        ui::manage_guest_menu();

        let mut choice = String::new();
        std::io::stdin().read_line(&mut choice)?;
        let choice = choice.trim();

        match choice {
            "1" => {
                println!("\n======= Reset Guest PIN =======");
                if let Err(e) = reset_guest_pin(conn, owner_id, homeowner_username) {
                    println!("Error: {}", e);
                }
            }
            "2" => {
                println!("\n======= Enable/Disable Guest =======");
                println!("[1] Enable Guest");
                println!("[2] Disable Guest");
                print!("Select an option [1-2]: ");
                std::io::stdout().flush().ok();

                let mut sub_choice = String::new();
                std::io::stdin().read_line(&mut sub_choice).ok();
                let sub_choice = sub_choice.trim();

                match sub_choice {
                    "1" => {
                        if let Err(e) = enable_guest(conn, owner_id, homeowner_username) {
                            println!("Error: {}", e);
                        }
                    }
                    "2" => {
                        if let Err(e) = disable_guest(conn, owner_id, homeowner_username) {
                            println!("Error: {}", e);
                        }
                    }
                    _ => println!("Invalid sub-option."),
                }
            }
            "3" => {println!("\n======= Delete Guest =======");
        if let Err(e) = delete_guest(conn, owner_id, homeowner_username) {
            println!("Error: {}", e);
        }
    }
            "4" => {
                println!("Returning to Homeowner Menu...");
                break;
            }
            _ => println!("Invalid choice, please enter 1–3."),
        }

        println!("\nPress ENTER to continue...");
        let mut dummy = String::new();
        std::io::stdin().read_line(&mut dummy).ok();
    }

    Ok(())
}

