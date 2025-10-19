use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
}; //Argon2 hashing algorithm for hashing and verification
use regex::Regex; // validating user inputs like usernames and passwords
use rpassword::read_password; // hidden password entry for CLI
use std::io::{self, Write}; // reading inputs and printing prompts
use zeroize::Zeroize; // used for sensitive data are wiped from the memory after use
use rusqlite::{params, Connection, OptionalExtension}; // handle for executing SQL queries

use crate::db::{get_user_id_and_role, user_exists};
use crate::logger::{init_logger_db, record_login_attempt, check_lockout, log_event, fake_verification_delay};

/*------------------------ Registration---------------------*/

/// Register a new account with role-based access control.
/// Admins can create any user type.
/// Technicians and Homeowners can create *only Guests*.
/// Guests cannot register anyone.
pub fn register_user(conn: &mut Connection, acting_user: Option<(&str, &str)>) -> Result<()> {
    // Identify acting user and role
    let (acting_username, acting_role) = match acting_user {
        Some((u, r)) => (u, r),
        None => {
            println!("Anonymous or guest context — registration not permitted.");
            return Ok(());
        }
    };

    // Permission enforcement
    match acting_role {
        "guest" => {
            println!("Guests cannot register new users.");
            return Ok(());
        }
        "homeowner" | "technician" | "admin" => {} // allowed
        _ => {
            println!("Invalid role '{acting_role}' — access denied.");
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
    if user_exists(conn, username)? {
        println!("Username '{}' already exists.", username);
        return Ok(());
    }

    // Determine the new user’s role based on who is creating it
    let new_role = match acting_role {
        "admin" => {
            // Admins can create any valid role type
            print!("Enter role [homeowner | technician | guest]: ");
            io::stdout().flush().ok();
            let mut role_input = String::new();
            if io::stdin().read_line(&mut role_input).is_err() {
                println!("Failed to read role input.");
                return Ok(());
            }
            let r = role_input.trim().to_lowercase();
            if r.is_empty() {
                "guest".to_string()
            } else {
                r
            }
        }
        "technician" | "homeowner" => {
            println!("{acting_role}s may only create guest accounts.");
            "guest".to_string()
        }
        _ => unreachable!(), // already validated above
    };

    // Validate role choice
    if !role_is_valid(&new_role) {
        println!("Invalid role type '{new_role}'.");
        return Ok(());
    }

    // Prompt for credential (password or PIN)
    let credential_label = if new_role == "guest" { "PIN" } else { "Password" };

    print!("Enter {credential_label}: ");
    io::stdout().flush().ok();
    let mut password = String::new();
    if io::stdin().read_line(&mut password).is_err() {
        println!("Failed to read {credential_label}.");
        return Ok(());
    }
    let password = password.trim().to_string();

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
    // Confirm password/PIN
    print!("Confirm {credential_label}: ");
    io::stdout().flush().ok();
    let mut confirm = String::new();
    if io::stdin().read_line(&mut confirm).is_err() {
        println!("Failed to read confirmation input.");
        return Ok(());
    }
    if confirm.trim() != password {
        println!("{credential_label}s do not match.");
        let mut p = password;
        p.zeroize();
        return Ok(());
    }

    // If a homeowner is creating a guest, link them via homeowner_id 
    let homeowner_id_opt = if acting_role == "homeowner" {
        if let Some((homeowner_username, _)) = acting_user {
            match get_user_id_and_role(conn, homeowner_username)? {
                Some((id, status)) if status == "homeowner" => Some(id),
                _ => {
                println!("Acting user is not a valid homeowner.");
                return Ok(());
            }
        }
    } else { None }
} else { None };

    // Hash and insert
    let hashed = match hash_password(&password) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to hash {credential_label}: {e}");
            return Ok(());
        }
    };
    let mut pw_clear = password;
    pw_clear.zeroize();

    let tx = conn.transaction()?;
    match tx.execute(
        "INSERT INTO users (username, hashed_password, user_status, homeowner_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))",
        params![username, hashed, new_role, homeowner_id_opt],
    ) {
        Ok(_) => {
            tx.commit()?;
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
    let salt = SaltString::generate(&mut OsRng); //generate unique random salt
    let hasher = argon2_hasher(); //create argon2id hasher instance
    let phc = hasher
        .hash_password(password.as_bytes(), &salt) // convert pwd to raw bytes and salt adds entropy and uniqueness
        .context("Failed to hash password")?;
    Ok(phc.to_string()) // Convert pwd hash to string (sutiable for storage in db)
}

fn role_is_valid(role: &str) -> bool {
    matches!(role, "homeowner" | "guest" | "technician")
}

/*-------------------------------------LOGIN----------------------*/

// Handle login for all user roles (admin, homeowner, technician, guest).
pub fn login_user(conn: &Connection, logger_con: &Connection) -> Result<Option<(String, String)>> {
    
    let logger_conn = init_logger_db().context("Failed to initialize logger")?;

    // Prompt for username
    print!("Username: ");
    io::stdout()
        .flush()
        .context("Failed to flush stdout while asking for username")?;

    let mut username_input = String::new(); // buffer for username input
    io::stdin()
        .read_line(&mut username_input)
        .context("Failed to read username input")?;

    let username = username_input.trim().to_string(); // keep original case for storage
    if username.is_empty() {
        eprintln!("Username is required.");
        return Ok(None);
    }
// 
    if check_lockout(logger_con, &username)? {
        return Ok(None);
    }

    // Prompt for password 
    print!("Password: ");
    io::stdout()
        .flush()
        .context("Failed to flush stdout while asking for password")?;

    let mut password = read_password().context("Failed to read password input")?;
    let password = password.trim().to_string();

    // Fetch stored hash and role (case-insensitive username lookup)
    let row = conn
        .query_row(
            "SELECT hashed_password, user_status, is_active FROM users WHERE username = ?1 COLLATE NOCASE",
            params![username],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1), r.get::<_, i64>(2)?)),
        )
        .optional()?;

    match row {
        Some((stored_hash, role_raw, is_active)) => {

            if is_active != 1 {
                println!("Account {} is currently disabled. Please contact administrator.", username);

                log_event(&logger_conn, &username, Some(&username), "ACCOUNT_DISABLED", Some("Login blocked"),)?;
                return Ok(None);
            }
            let ok = verify_password(&password, &stored_hash)?;
            let mut p = password;
            p.zeroize();

            if ok {
                // Record successful login
                record_login_attempt(&logger_conn, &username, true)?;
                let role = role_raw?.trim().to_lowercase(); // normalize role for logic

                // Update last login timestamp
                conn.execute(
                    "UPDATE users 
                     SET last_login_time = datetime('now'), 
                         updated_at = datetime('now') 
                     WHERE username = ?1 COLLATE NOCASE",
                    params![username],
                )?;

                // Friendly role-based message
                match role.as_str() {
                    "admin" => println!("🔧 Welcome, Admin! Accessing control panel..."),
                    "homeowner" => println!("🏠 Welcome home, {username}!"),
                    "technician" => println!("🧰 Welcome, {username}!"),
                    "guest" => println!("👋 Welcome, {username}!"),
                    _ => println!("Unknown role! Please contact an administrator!"),
                }

                return Ok(Some((username, role)));
            } else {
                // Record failed login
                record_login_attempt(&logger_conn, &username, false)?;
                Ok(None)
            }
        }

        None => {
            // No such user - simulate delay to prevent timing attacks
            fake_verification_delay();
            record_login_attempt(&logger_conn, &username, false)?;
            let mut p = password;
            p.zeroize();
            Ok(None)
        }
    }
}

// Verify a password against a stored PHC hash
pub fn verify_password(password: &str, stored_hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(stored_hash).context("Invalid password hash format")?; // parse stored hash
    let hasher = argon2_hasher(); // create argon2id hasher instance
    Ok(hasher.verify_password(password.as_bytes(), &parsed).is_ok()) // return true if verified
}

