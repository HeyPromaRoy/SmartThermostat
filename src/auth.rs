use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng as argonOsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2}; //Argon2 hashing algorithm for hashing and verification
use lazy_static::lazy_static;
use regex::Regex; // validating user inputs like usernames and passwords
use rpassword::read_password; // hidden password entry for CLI
use std::{sync::{Arc, Mutex}, io::{self, Write}}; // reading inputs and printing prompts
use zeroize::{Zeroize, Zeroizing}; // used for sensitive data are wiped from the memory after use
use rusqlite::{params, Connection, OptionalExtension}; // handle for executing SQL queries

use crate::db;
use crate::logger;


/*------------------------ Registration---------------------*/

/* Register a new account with role-based access control.
  Admins can create any user type.
  Homeowners can create *only Guests*.
  Technicians can only manage existing guests.
  Guests cannot register anyone. */
pub fn register_user(conn: &mut Connection, acting_user: Option<(&str, &str)>) -> Result<()> {
    // Identify acting user and role
    let (acting_username, _) = match acting_user {
        Some((u, r)) => (u, r),
        None => {
            println!("Anonymous or guest context — registration not permitted.");
            return Ok(());
        }
    };

    // Read authoritative status from DB (role, is_active)
    let (acting_role, is_active): (String, i64) = conn
        .query_row(
            "SELECT user_status, COALESCE(is_active,1) FROM users WHERE username = ?1 COLLATE NOCASE",
            params![acting_username],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .map(|v| v)
        .unwrap_or_else(|| ("guest".to_string(), 0));

    if is_active != 1 {
        println!("Acting account is disabled.");
        return Ok(());
    }

    match acting_role.as_str() {
        "guest" => {
            println!("Guests cannot register new users.");
            return Ok(());
        }
        "homeowner" | "technician" | "admin" => {}
        _ => {
            println!("Invalid acting role '{}'.", acting_role);
            return Ok(());
        }
    }

    //Get username for the new account
    print!("Enter new username (3–32 chars, letters/digits/_ only): ");
    io::stdout().flush().ok();
    let mut username = String::new();
    if io::stdin().read_line(&mut username).is_err() {
        println!("Failed to read username input.");
        return Ok(());
    }
    let username = username.trim();

    if !username_is_valid(username) {
        println!("Invalid username format.");
        return Ok(());
    }
    if db::user_exists(conn, username)? {
        println!("Username '{}' already exists.", username);
        return Ok(());
    }

    // Determine the new user’s role based on who is creating it
    let new_role = match acting_role.as_str() {
        "admin" => {
            // Admins can create any valid role type
            print!("Enter role [homeowner | technician]: ");
            io::stdout().flush().ok();
            let mut role_input = String::new();
            if io::stdin().read_line(&mut role_input).is_err() {
                println!("Failed to read role input.");
                return Ok(());
            }
            let r = role_input.trim().to_lowercase();
           match r.as_str() {
            "homeowner" | "technician" => r, // allowed roles
            _ => {
                println!("Invalid role. Admins can only create homeowners or technicians.");
                return Ok(()); // stop registration here
                }
            }
        }
        "homeowner" | "technician" => {
        println!("{acting_role}s may only create guest accounts.");
        "guest".to_string()
    }
        _ => unreachable!(), // already validated above
    };

    if new_role == "admin" {
        println!("Creation of admin accounts is disabled");
        return Ok(());
        }

    // Validate role choice
    if !role_is_valid(&new_role) {
        println!("Invalid role type '{new_role}'.");
        return Ok(());
    }

    // Prompt for credential (password or PIN)
    let credential_label = if new_role == "guest" { "PIN" } else { "Password" };

    print!("Enter {credential_label}: ");
    io::stdout().flush().ok();
    let password = {
        let raw = read_password()?;
        Zeroizing::new(raw.trim_end_matches(['\r', '\n']).to_string())
    };


    // Hard cap to prevent resource abuse (e.g., extremely long inputs)
    const MAX_SECRET_LEN: usize = 1024;
    if password.len() > MAX_SECRET_LEN {
        println!("{} too long (max {}).", credential_label, MAX_SECRET_LEN);
        return Ok(());
    }

        // If registering a guest, enforce PIN policy
    if new_role == "guest" {
    // numeric-only, min 6 digits. Adjust MIN_PIN_LEN to taste.
    const MIN_PIN_LEN: usize = 6;
    if password.len() < MIN_PIN_LEN || !password.chars().all(|c| c.is_ascii_digit()) {
        println!(
            "Invalid PIN. PIN must be numeric and at least {} digits long.",
            MIN_PIN_LEN
        );
        let mut p = password;
        p.zeroize();
        return Ok(());
        }
    } else {
    // Password strength validation for non-guests as before
    if !password_is_strong(&password, username) {
        let mut p = password;
        p.zeroize();
        return Ok(());
    }
}


    if password.is_empty() {
        println!("{credential_label} cannot be empty.");
        return Ok(());
    }

    // Enforce strong password (non-guests only)
    if new_role != "guest" && !password_is_strong(&password, username) {
        let mut p = password;
        p.zeroize();
        return Ok(());
    }



    // Confirm password/PIN
    print!("Confirm {credential_label}: ");
    io::stdout().flush().ok();
    let confirm = {
        let raw = read_password()?;
        Zeroizing::new(raw.trim_end_matches(['\r', '\n']).to_string())
    };

    if confirm.as_str() != password.as_str() {
        println!("{credential_label}s do not match.");
    // `password` and `confirm` are wiped on drop
        return Ok(());
    }

    // If a homeowner is creating a guest, link them via homeowner_id 
    let homeowner_id_opt = if new_role == "guest" {
    match acting_role.as_str() {
        // homeowners link guests to themselves
        "homeowner" => {
            match db::get_user_id_and_role(conn, acting_username)? {
                Some((id, status)) if status == "homeowner" => Some(id),
                _ => {
                    println!("Acting user is not a valid homeowner.");
                    return Ok(());
                }
            }
        }
        // Technicians can register guests *only if they have permission* under a homeowner
        "technician" => {
            // Find which homeowner this technician has permission for
            let homeowner_username_opt: Option<String> = conn
                .query_row(
                    r#"
                    SELECT homeowner_username FROM technician_jobs
                     WHERE technician_username = ?1 COLLATE NOCASE AND status IN ('ACCESS_GRANTED','TECH_ACCESS')
                       AND datetime(updated_at, printf('+%d minutes', access_minutes)) > datetime('now')
                     ORDER BY updated_at DESC
                     LIMIT 1
                    "#,
                    params![acting_username],
                    |r| r.get(0),
                )
                .optional()?;

            let Some(homeowner_username) = homeowner_username_opt else {
                println!("Technician '{acting_username}' has no active homeowner access grants.");
                return Ok(());   
            };

            if !db::tech_has_perm(conn, acting_username, &homeowner_username)? {
                println!("Technician '{acting_username}' does not currently have permission under homeowner '{homeowner_username}'.");
                return Ok(());
            }
            // Retrieve homeowner ID for linkage
            match db::get_user_id_and_role(conn, &homeowner_username)? {
                Some((id, status)) if status == "homeowner" => Some(id),
                _ => {
                    println!("Failed to resolve homeowner ID for '{}'.", homeowner_username);
                    return Ok(());
                }
            }
        }

            _ => None,
            }
        } else {
    None
    };
    

    // Hash and insert
    let hashed = match hash_password(&password) {
        Ok(h) => h,
        Err(_) => {
            eprintln!("Failed to hash {credential_label}");
            return Ok(());
        }
    };
    let mut pw_clear = password;
    pw_clear.zeroize();

    match db::insert_user(conn, &username, &acting_username, &hashed, &new_role, homeowner_id_opt) {
        Ok(_) => {
            println!("Registered '{username}' as {new_role}");
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.to_lowercase().contains("unique") {
                println!("Username already exists.");
            } else {
                println!("Registration failed: {msg}");
            }
        }
    }

    Ok(())
}


// Validates a username format (no special characters)
pub fn username_is_valid(username: &str) -> bool {
    // Ensure no whitespace or control characters
    if username.chars().any(|c| c.is_whitespace() || c.is_control()) {
        eprintln!("Username contains spaces or control characters");
        return false;
    }
    //non-ASCII characters
    if !username.is_ascii() {
        eprintln!("Username contains non-ASCII characters.");
        return false;
    }
    // validate allowed characters and length using regex
    match Regex::new(r"^[A-Za-z0-9_]{3,32}$") {
        Ok(re) => {
            if !re.is_match(username) { // if username do not match, invalid
                eprintln!("Invalid username: only letters, digits, and underscores are allowed (3–32 chars).");
                return false;
            }
            true
        }
        Err(err) => { //handle unexpected regex failure without panic
            eprintln!("Internal regex error: {}", err);
            false
        }
    }
}

// Validates password strength (upper, lower, digit, special)
fn password_is_strong(password: &str, username: &str) -> bool {
    if password.to_lowercase().contains(&username.to_lowercase()) {
        eprintln!("Password should not contain the username.");
        return false;
    }

    if password.len() < 8 {
        println!("Password too short (minimum 8 characters).");
        return false;
    }
    //Compile regex patterns safely
    let upper_reg = Regex::new(r"[A-Z]");
    let lower_reg = Regex::new(r"[a-z]");
    let digit_reg = Regex::new(r"\d");
    let special_reg = Regex::new(r"[@$!%*?&\-_#]");

    // Handling compilation errors
    let (has_upper, has_lower, has_digit, has_special) = match (upper_reg, lower_reg, digit_reg, special_reg) {
        (Ok(u), Ok(l), Ok(d), Ok(s)) => (
            u.is_match(password),
            l.is_match(password),
            d.is_match(password),
            s.is_match(password),
        ),
        _ => {
            eprintln!("Internal regex error: password validation unavailable.");
            return false;
        }
    };
    // Enforce strength requirements
    if !(has_upper && has_lower && has_digit && has_special) {
        eprintln!(
            "Weak password. Must include at least:
                    • 1 uppercase letter
                    • 1 lowercase letter
                    • 1 digit
                    • 1 special character (@$!%*?&_-#)"
        );
        return false;
    }
    true
}

// Build a secure Argon2id hasher with reasonable parameters
// Argon2id is chosen for its hybrid resistance (safe against both GPU and side-channel attacks).
/// We use a memory-hard setup that balances performance and security for modern CPUs.
fn argon2_hasher() -> Argon2<'static> {
    /* Create Argon2 hashing parameters:
       - memory_cost: 65_536 KiB (≈64 MiB) → resists GPU cracking
       - iterations: 3 passes over memory
       - parallelism: 1 thread (sufficient for most single-user systems)
       - output_length: None → use default (32 bytes) */
    let params = argon2::Params::new(65_536, 3, 1, None).expect("Invalid Argon2 params");
    Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
}

// Hash a plaintext password securely using Argon2id securely
// Returns a Password Hashing Competition) formatted string
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut argonOsRng); //generate unique random salt
    let hasher = argon2_hasher(); //create argon2id hasher instance
    let phc = hasher
        .hash_password(password.as_bytes(), &salt) // convert pwd to raw bytes and salt adds entropy and uniqueness
        .context("Failed to hash password")?;
    Ok(phc.to_string()) // Convert pwd hash to string (sutiable for storage in db)
}

fn role_is_valid(role: &str) -> bool {
    matches!(role, "homeowner" | "guest" | "technician")
}



// ===============================================================
//                         LOGIN FUNCTIONS
// ===============================================================

lazy_static! {
    // One active session per running instance (CLI process)
    pub static ref ACTIVE_SESSION: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
}

pub fn login_user(conn: &Connection) -> Result<Option<(String, String)>> {
    // Single in-process session guard
   { 
    let active = ACTIVE_SESSION
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

    // Prompt username
    print!("Username: ");
    io::stdout().flush().ok();
    let mut username_input = String::new();
    io::stdin().read_line(&mut username_input)?;
    let username = username_input.trim().to_string();
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


    // Prompt password (hidden input)
    print!("Password: ");
    io::stdout().flush().ok();
    let pw_in = Zeroizing::new(read_password()?);
    let password = pw_in.trim_end_matches(['\r', '\n']); // &str view; buffer wiped on drop

    // Fetch stored hash + role + active flag
    let row = conn
        .query_row(
            "SELECT hashed_password, user_status, is_active
             FROM users WHERE username = ?1 COLLATE NOCASE",
            params![username],
            |r| Ok((
                r.get::<_, String>(0)?, // hash
                r.get::<_, String>(1)?, // role
                r.get::<_, i64>(2)?,    // is_active
            )),
        )
        .optional()?;

    // Constant-time-ish behavior for unknown users
    let fake_hash = "$argon2id$v=19$m=65536,t=3,p=1$ABCdef123Q$hR2eWkj4jvIY6MfGfQ/fZg";
    if row.is_none() {
        let _ = verify_password(password, fake_hash);
        logger::fake_verification_delay();
        logger::increment_session_fail(conn, None)?;
        logger::session_lockout_check(conn, None)?;
        logger::record_login_attempt(conn, &username, false)?;
        println!("Invalid username or password.");
        return Ok(None);
    }

    let (stored_hash, role, is_active) =
        row.ok_or_else(|| anyhow::anyhow!("Missing user record after fetch"))?;

    // Verify password FIRST (avoid status-based enumeration)
    if !verify_password(password, &stored_hash)? {
        logger::fake_verification_delay();
        logger::increment_session_fail(conn, Some(&username))?;
        logger::session_lockout_check(conn, Some(&username))?;
        logger::record_login_attempt(conn, &username, false)?;
        println!("Invalid username or password.");
        return Ok(None);
    }


    if is_active != 1 {
        println!("Account disabled. Please contact administrator.");
        let _ = logger::log_event(
            conn,
            &username,
            Some(&username),
            "ACCOUNT_DISABLED",
            Some("Blocked login on disabled account"),
        );
        return Ok(None);
    }

    // On success: reset anonymous lockout counters
    conn.execute(
        "UPDATE session_state
         SET failed_attempts = 0, is_locked = 0, locked_until = NULL
         WHERE username IS NULL",
        [],
    )?;

        // cleanup of expired sessions
    let _ = conn.execute(
        "DELETE FROM session_state WHERE session_expires <= datetime('now')",
        [],
    );

    // Deny concurrent login if a live session already exists
    let has_live_session: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM session_state
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
    

    // Success: record, create new session (stores only hash; returns plaintext token)
    db::end_session(conn, "")?;
    logger::record_login_attempt(conn, &username, true)?;
    let _session_token_plain = db::update_session(conn, Some(&username))?;
    
    // reflect session in this process (CLI)
    let mut active = ACTIVE_SESSION
    .lock()
    .map_err(|_| anyhow::anyhow!("Failed to acquire ACTIVE_SESSION lock"))?;
    *active = Some(username.clone());
    drop(active);

    Ok(Some((username, role)))
}

// Verify a password against a stored PHC hash
pub fn verify_password(password: &str, stored_hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(stored_hash).context("Invalid password hash format")?; // parse stored hash
    let hasher = argon2_hasher(); // create argon2id hasher instance
    Ok(hasher.verify_password(password.as_bytes(), &parsed).is_ok()) // return true if verified
}

pub fn logout_user(conn: &Connection) -> Result<()> {
    // Check active session in memory
    let mut active_guard = ACTIVE_SESSION
        .lock()
        .map_err(|_| anyhow::anyhow!("Failed to acquire ACTIVE_SESSION lock"))?;

    let username = match &*active_guard {
        Some(u) => u.clone(),
        None => {
            println!("No user is currently logged in.");
            return Ok(());
        }
    };

    //End the session in DB
    if let Err(_) = db::end_session(conn, &username) {
        eprintln!("Warning: failed to end DB session.");
    }

    // Log the logout event
    if let Err(_) = logger::log_event(conn, &username, Some(&username), "LOGOUT", Some("User logged out")) {
        eprintln!("Warning: failed to record logout event");
    }


    //Clear memory safely
    (*active_guard).take(); // sets ACTIVE_SESSION = None
    drop(active_guard);     // release lock

    println!("User '{}' logged out successfully.", username);

    Ok(())
}
