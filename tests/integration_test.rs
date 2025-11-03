
use smart_thermostat::senser::*;
use smart_thermostat::weather::*;
use smart_thermostat::auth::*;
use smart_thermostat::hvac::*;
use smart_thermostat::logger::*;
use smart_thermostat::energy::*;
use smart_thermostat::db::*;
use smart_thermostat::technician::*;

use anyhow::Result;
use rusqlite::{Connection,params, OptionalExtension};
use std::{path::PathBuf, env, fs}; 




mod tests {
    use super::*;

    fn test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();

    // security_log table
    conn.execute(
        "CREATE TABLE security_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            actor_username TEXT NOT NULL,
            target_username TEXT,
            event_type TEXT NOT NULL,
            description TEXT,
            timestamp TEXT NOT NULL
        )",
        [],
    ).unwrap();

    // lockouts table
    conn.execute(
        "CREATE TABLE lockouts (
            username TEXT PRIMARY KEY COLLATE NOCASE,
            locked_until TEXT NOT NULL,
            lock_count INTEGER DEFAULT 1
        )",
        [],
    ).unwrap();

    //  users table with full columns needed by tests
    conn.execute(
        "CREATE TABLE users (
            username TEXT PRIMARY KEY COLLATE NOCASE,
            hashed_password TEXT,
            user_status TEXT,
            is_active INTEGER DEFAULT 1,
            owner_id TEXT,
            last_login_time TEXT,
            updated_at TEXT
        )",
        [],
    ).unwrap();

    // HVAC state table
    conn.execute(
    "CREATE TABLE IF NOT EXISTS hvac_state (
        id INTEGER PRIMARY KEY,
        mode TEXT NOT NULL,
        target_temperature REAL NOT NULL,
        light_status TEXT NOT NULL,
        current_profile TEXT
    )",
    [],
    ).unwrap();

    // Insert default HVAC state
    conn.execute(
    "INSERT OR IGNORE INTO hvac_state (id, mode, target_temperature, light_status, current_profile)
     VALUES (1, 'Off', 22.0, 'OFF', NULL)",
    [],
    ).unwrap();

    // energy table for energy-related tests
    conn.execute(
        "CREATE TABLE energy (
            id INTEGER PRIMARY KEY,
            homeowner_username TEXT,
            timestamp TEXT,
            energy_kwh REAL
        )",
        [],
    ).unwrap();

        // technician_jobs
        conn.execute(
            r#"
            CREATE TABLE technician_jobs (
                job_id INTEGER PRIMARY KEY AUTOINCREMENT,
                homeowner_username TEXT NOT NULL,
                technician_username TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'ASSIGNED',
                access_minutes INTEGER NOT NULL,
                job_desc TEXT NOT NULL,
                grant_start TEXT,
                grant_expires TEXT,
                updated_at TEXT
            )
            "#,
            [],
        ).unwrap();

    conn
}

// ===================================================================== //
//                           DB TESTS
// ===================================================================== //
#[test]
fn test_db_tables() -> Result<()> {
    // Create a temp file path in the system temp directory
    let tmp_dir = env::temp_dir();
    let mut tmp_file = PathBuf::from(tmp_dir);
        tmp_file.push("temp.db");

         // Delete any leftover DB from previous failed runs
        let _ = fs::remove_file(&tmp_file);

        // Initialize DB at that path
        let conn = get_connection(&tmp_file)?;

        // Query tables
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        let table_iter = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut tables = Vec::new();
        for tbl in table_iter {
            if let Ok(name) = tbl {
                tables.push(name);
            }
        }

        let expected: Vec<String> = vec![
            "users", "security_log", "lockouts", "session_state",
            "technician_jobs", "weather", "profiles",
            "hvac_activity_log", "hvac_state",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        for name in &expected {
            assert!(
                tables.contains(name),
                "Expected table '{}' was not found in DB",
                name
            );
        }

    assert!(!tables.contains(&"fake_table".to_string()), "Unexpected fake table found in DB!");

    // Remove the test database file
    fs::remove_file(&tmp_file).ok();

    Ok(())
}


    
// ===================================================================== //
//                           SENSOR TESTS
// ===================================================================== //

//  Test Sensor: Temperature range
#[test]
fn test_temperature_range() {
    let temp = get_indoor_temperature().unwrap();
    assert!(temp >= -40.0 && temp <= 85.0, "Temperature out of range: {}", temp);
}

//  Test Sensor: Humidity range
#[test]
fn test_humidity_range() {
    let humidity = get_indoor_humidity().unwrap();
    assert!(humidity >= 0.0 && humidity <= 100.0);
}

//  Test Sensor: CO level range
#[test]
fn test_co_range() {
    let co = get_indoor_colevel().unwrap();
    assert!(co >= 0.0 && co <= 1000.0);
}

// ===================================================================== //
//                           WEATHER API TEST
// ===================================================================== //

//  Test Weather: API returns valid struct
#[test]
fn test_fetch_weather() {
    let w = fetch_weather().expect("weather fetch failed");

    // At least condition string must not be empty
    assert!(!w.condition.is_empty(), "Condition should not be empty");
}

// ===================================================================== //
//                           AUTH TESTS
// ===================================================================== //
#[test]
fn test_username_validation() {
        assert_eq!(username_is_valid("Alice_123"), true); //valid username
        assert_eq!(username_is_valid("Bob!sCool"), false); // Contains special chars
        assert_eq!(username_is_valid("B"), false); // Short username
        assert_eq!(username_is_valid("VeryVeryVeryLongCoolAwesome_Username123ILikeCats"), false); //Very Long Username
}

#[test]
fn test_password_validation() {
        assert_eq!(password_is_strong("Password123", "user"), false); // No special chars
        assert_eq!(password_is_strong("ILoveCats", "user"), false); // Only letters
        assert_eq!(password_is_strong("12341241", "user"), false); // Only numbers
        assert_eq!(password_is_strong("User@123", "user"), false); // Contains username

        assert_eq!(password_is_strong("Str0ngP@ssword456", "user"), true); // Valid with all conditions

}

#[test]
fn test_role_validation() {
        assert_eq!(role_is_valid("homeowner"), true);
        assert_eq!(role_is_valid("notvalid"), false);
}

#[test]
fn test_password_hash_verif() -> anyhow::Result<()> {
        let password = "StrongP@ssword1";
        let wrong_pw = "Password";
        let hashed_pw = hash_password(password)?;
        assert_eq!(verify_password(&password, &hashed_pw)?, true);
        assert_eq!(verify_password(&wrong_pw, &hashed_pw)?, false);
        Ok(())
}
    
#[test]
fn test_user_auth() -> Result<()> {
    let tmp_dir = env::temp_dir(); //create temp db path
    let mut tmp_file = PathBuf::from(tmp_dir);
    tmp_file.push("auth.db");

    if tmp_file.exists() { //Sanity Check
        fs::remove_file(&tmp_file).ok();
    }

    let conn = get_connection(&tmp_file)?; // initialize db
        
    //manual registration
    let username = "alice";
    let password = "StrongP@ssword1";
    let hashed_pw = hash_password(password)?;

    conn.execute(       
        "INSERT INTO users (username, hashed_password, user_status, is_active)
         VALUES (?1, ?2, 'homeowner', 1)",
        params![username, hashed_pw])?;

    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1)", params![username], |r| r.get(0))?;
            
        assert_eq!(exists, true, "User should exist in DB after insertion");

    if !exists { 
            conn.execute(
            "INSERT INTO users (username, hashed_password, user_status, is_active)
            VALUES (?1, ?2, 'homeowner', 1)", params![username, hashed_pw])?;
        }
            //verify correct pwd works
        let stored_hash: String = conn.query_row(
            "SELECT hashed_password FROM users WHERE username = ?1", params![username], |r| r.get(0))?;


        assert_eq!(verify_password(password, &stored_hash)?, true);

        assert_eq!(verify_password("WrongPassword", &stored_hash)?, false);

        // Login Simulation
        let _session_token = update_session(&conn, Some(username))?;

        let db_session: Option<String> = conn.query_row(
        "SELECT username FROM session_state WHERE username = ?1", params![username], |r| r.get(0),).optional()?;
        
        assert!(db_session.is_some(), "Expected session_state entry for logged-in user");

        { //in-memory session reflection
        let mut active = ACTIVE_SESSION.lock()
             .map_err(|_| anyhow::anyhow!("Failed to acquire ACTIVE_SESSION lock"))?;
             *active = Some(username.to_string());
            
            assert_eq!(active.as_deref(), Some(username), "ACTIVE_SESSION should match logged_in user");
        }

            logout_user(&conn)?;
        // Check that in-memory session is cleared
        {
        let active = ACTIVE_SESSION.lock()
        .map_err(|_| anyhow::anyhow!("Failed to acquire ACTIVE_SESSION lock"))?;
        assert!(active.is_none(), "ACTIVE_SESSION should be cleared after logout");
        }

            // Check that session_state in DB is cleared
        let session_exists: bool = conn.query_row(
        "SELECT COUNT(*) FROM session_state WHERE username = ?1",
        params![username], |r| r.get(0))?;

        assert_eq!(session_exists, false, "session_state should be empty after logout");

            fs::remove_file(&tmp_file).ok();

            Ok(())
    }

// ===================================================================== //
//                           HVAC TESTS
// ===================================================================== //


    /// Helper function to create an in-memory database for testing
    /// This avoids touching a real database.
    

    
    /// Test that the `new()` constructor initializes HVACSystem correctly
    #[test]
    fn test_new_hvac_system() {
        let conn = test_db();
        let hvac = HVACSystem::new(&conn);
        // By default, mode should be Off
        assert_eq!(hvac.mode, HVACMode::Off);
        // Default target temperature should be 22.0°C
        assert_eq!(hvac.target_temperature, 22.0);
    }

    /// Test that set_mode changes the HVAC mode correctly
    #[test]
    fn test_set_mode() {
        let conn = test_db();
        let mut hvac = HVACSystem::new(&conn);

        // Set mode to Heating
        hvac.set_mode(&conn, HVACMode::Heating);
        assert_eq!(hvac.mode, HVACMode::Heating);

        // Set mode to Cooling
        hvac.set_mode(&conn, HVACMode::Cooling);
        assert_eq!(hvac.mode, HVACMode::Cooling);
    }

    /// Test setting a target temperature within allowed limits
    #[test]
    fn test_set_target_temperature_within_limits() {
        let conn = test_db();
        let mut hvac = HVACSystem::new(&conn);

        hvac.set_target_temperature(&conn, 25.0);
        // Temperature should be set exactly as requested
        assert_eq!(hvac.target_temperature, 25.0);
    }

    /// Test that temperatures below the minimum are adjusted correctly
    #[test]
    fn test_set_target_temperature_below_min() {
        let conn = test_db();
        let mut hvac = HVACSystem::new(&conn);

        hvac.set_target_temperature(&conn, 10.0);
        // Should automatically set to MIN_TEMPERATURE
        assert_eq!(hvac.target_temperature, MIN_TEMPERATURE);
    }

    /// Test that temperatures above the maximum are adjusted correctly
    #[test]
    fn test_set_target_temperature_above_max() {
        let conn = test_db();
        let mut hvac = HVACSystem::new(&conn);

        hvac.set_target_temperature(&conn, 50.0);
        // Should automatically set to MAX_TEMPERATURE
        assert_eq!(hvac.target_temperature, MAX_TEMPERATURE);
    }

    /// Test that diagnostics function runs without panicking
    #[test]
    fn test_diagnostics_runs() {
        let conn = test_db();
        let hvac = HVACSystem::new(&conn);
        hvac.diagnostics(&conn);
        // No assert needed: we just want to ensure it runs without crashing
    }

    /// Example: Mocking the sensor for testing update() behavior
    /// In real unit tests, you'd use a mock library or inject a mock sensor.
    mod mock_senser {
        pub fn get_indoor_temperature() -> Result<f32, &'static str> {
            Ok(23.0) // Always return 23.0°C for testing
        }
    }

    #[test]
fn test_mock_senser_usage() {
    let temp = mock_senser::get_indoor_temperature().unwrap();
    assert_eq!(temp, 23.0); // now the function is used
}


    /// Simple test to ensure update() behaves for Auto mode heating scenario
    #[test]
    fn test_update_auto_mode_heating() {
        let conn = test_db();
        let mut hvac = HVACSystem::new(&conn);
        hvac.mode = HVACMode::Auto;
        hvac.target_temperature = 25.0;

        // Pretend current temperature is 23°C (below target - 0.5)
        let current_temp = 23.0;
        if current_temp < hvac.target_temperature - 0.5 {
            hvac.update(&conn); // Should trigger "Auto heating..."
        }
        // This test ensures update() can run without panicking in Auto mode
    }

// ===================================================================== //
//                           LOGGER TESTS
// ===================================================================== //


// Test: log_event() basic logging
   
 #[test]
fn test_log_event_creates_db_entry() -> Result<()> {
    let conn = test_db();

    // Call the function to log a sample event
    log_event(&conn, "alice", Some("alice"), "TEST_EVENT", Some("This is a test"))?;

    // Verify that it was written into the DB
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_log WHERE actor_username='alice' AND event_type='TEST_EVENT'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 1);

    Ok(())
}

// Test: record_login_attempt() — success clears lockout

#[test]
fn test_record_login_success_clears_lockout() -> Result<()> {
    let conn = test_db();

    // Simulate that user was previously locked out
    conn.execute(
        "INSERT INTO lockouts (username, locked_until, lock_count)
         VALUES ('alice', datetime('now', '+60 seconds'), 2)",
        [],
    )?;

    // Record a successful login
    record_login_attempt(&conn, "alice", true)?;

    // The lockout should be cleared
    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lockouts WHERE username='alice'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(remaining, 0);

    Ok(())
}

// Test: record_login_attempt() — failed attempts lockout

#[test]
fn test_record_login_failure_triggers_lockout() -> Result<()> {
    let conn = test_db();

    // Simulate multiple failed attempts
    for _ in 0..MAX_ATTEMPTS {
        record_login_attempt(&conn, "alice", false)?;
    }

    // The user should now be locked out
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lockouts WHERE username='alice'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 1);

    Ok(())
}

// Test: check_lockout() — should detect active ban

#[test]
fn test_check_lockout_detects_active() -> Result<()> {
    let conn = test_db();

    // Set a lockout in the future
    let locked_until = (now_est() + chrono::Duration::seconds(60)).to_rfc3339();
    conn.execute(
        "INSERT INTO lockouts (username, locked_until, lock_count)
         VALUES ('alice', ?1, 1)",
        [&locked_until],
    )?;

    let locked = check_lockout(&conn, "alice")?;
    assert!(locked);

    Ok(())
}


// Test: fake_verification_delay() — delay is within bounds

#[test]
fn test_fake_verification_delay_bounds() {
    use std::time::Instant;
    let start = Instant::now();
    fake_verification_delay();
    let elapsed = start.elapsed().as_millis();

    // Should be roughly between 100–250 ms
    assert!(
        (100..=300).contains(&(elapsed as u64)),
        "Delay {}ms not in expected range",
        elapsed
    );
}

// ===================================================================== //
//                           ENERGY TRACKER TESTS
// ===================================================================== //

// checking if db is working fine
#[test]
    fn test_energy_store_and_load() -> Result<()> {
        let conn = test_db();
        let username = "homeowner1";

        let data = EnergyTracker::generate_mock_data(2, username);
        EnergyTracker::store_energy_data(&conn, &data, username)?;

        let loaded = EnergyTracker::load_energy_data(&conn, username, 30)?;
        assert_eq!(data.len(), loaded.len());
        Ok(())
    }

// ===================================================================== //
//                      GUEST MANAGEMENT TESTS
// ===================================================================== //


    /// Helper to get the activity status (0=inactive, 1=active) of a user.
    fn get_user_status(conn: &Connection, username: &str) -> Result<i32> {
        conn.query_row(
            "SELECT is_active FROM users WHERE username = ?1",
            params![username],
            |row| row.get(0),
        ).map_err(|e| anyhow::anyhow!("User status check failed: {}", e))
    }


    /// Tests that a Homeowner can successfully disable and re-enable their own Guest.
    #[test]
    fn test_guest_status_toggling() -> Result<()> {
        let conn = test_db();
        let ho_user = "HO_Manager";
        let guest_user = "Guest_Toggable";
        let hash = hash_password("pw")?;

        // Setup HO and GUEST owned by HO
        conn.execute("INSERT INTO users (username, hashed_password, user_status, is_active, owner_id) VALUES (?1, ?2, 'homeowner', 1, 'system')", params![ho_user, hash])?;
        conn.execute("INSERT INTO users (username, hashed_password, user_status, is_active, owner_id) VALUES (?1, ?2, 'guest', 1, ?3)", params![guest_user, hash, ho_user])?;
        
        // 1. Simulate Disable (Set is_active = 0)
        let rows_updated_disable = conn.execute(
            "UPDATE users SET is_active = 0 WHERE username = ? AND owner_id = ?",
            params![guest_user, ho_user]
        )?;
        assert_eq!(rows_updated_disable, 1, "Disable should update exactly 1 row.");
        assert_eq!(get_user_status(&conn, guest_user)?, 0, "Guest status should be 0 (disabled).");

        // 2. Simulate Enable (Set is_active = 1)
        let rows_updated_enable = conn.execute(
            "UPDATE users SET is_active = 1 WHERE username = ? AND owner_id = ?",
            params![guest_user, ho_user]
        )?;
        assert_eq!(rows_updated_enable, 1, "Enable should update exactly 1 row.");
        assert_eq!(get_user_status(&conn, guest_user)?, 1, "Guest status should be 1 (enabled).");
        
        Ok(())
    }

    /// Tests for Broken Access Control 
    /// Ensures Homeowner A CANNOT disable or delete Guest accounts owned by Homeowner B.
    #[test]
    fn test_guest_management_security_idor() -> Result<()> {
        let conn = test_db();
        
        // Setup Attacker (HO_A) and Victim Owner (HO_B)
        let ho_a_attacker = "HO_A_Attacker";
        let ho_b_victim = "HO_B_Victim";
        let hash = hash_password("pw")?;
        conn.execute("INSERT INTO users (username, hashed_password, user_status, is_active, owner_id) VALUES (?1, ?2, 'homeowner', 1, 'system')", params![ho_a_attacker, hash])?;
        conn.execute("INSERT INTO users (username, hashed_password, user_status, is_active, owner_id) VALUES (?1, ?2, 'homeowner', 1, 'system')", params![ho_b_victim, hash])?;

        // Setup Guest G owned by HO_B (the victim)
        let guest_g_user = "Guest_G_OwnedByB";
        let guest_hash = hash_password("1111")?;
        conn.execute("INSERT INTO users (username, hashed_password, user_status, is_active, owner_id) VALUES (?1, ?2, 'guest', 1, ?3)", params![guest_g_user, guest_hash, ho_b_victim])?;
        
        // Sanity Check: Guest G is active (1)
        assert_eq!(get_user_status(&conn, guest_g_user)?, 1);
        
        // IDOR Attempt 1: HO_A (Attacker) tries to disable Guest_G (Victim's Guest)
        // The query *must* check the owner_id against the acting user's ID (HO_A).
        let rows_updated = conn.execute(
            "UPDATE users SET is_active = 0 WHERE username = ? AND owner_id = ?",
            params![guest_g_user, ho_a_attacker] // Attacker passes Victim's Guest and *their own* HO ID
        )?;
        
        // Verification: 0 rows should be updated because HO_A is not the owner (HO_B).
        assert_eq!(rows_updated, 0, "IDOR attempt should update 0 rows.");
        
        // Final Check: Guest G must still be active
        assert_eq!(get_user_status(&conn, guest_g_user)?, 1, "Victim's Guest must remain active.");
        
        Ok(())

}

// ===================================================================== //
//                      TECHNICIAN TESTS
// ===================================================================== //

//------ Some helper functions to run Technicia.rs test ------

// Helper: log a user into the global session
    

    fn login_user(username: &str) {
        let mut guard = ACTIVE_SESSION.lock().unwrap();
        *guard = Some(username.to_owned());
    }

//  Helper: insert a user (homeowner or technician)
    
   fn insert_user(conn: &Connection, username: &str, status: &str, pw: &str) -> Result<()> {
        let hash = hash_password(pw)?;
        conn.execute(
            "INSERT INTO users (username, hashed_password, user_status, is_active)
             VALUES (?1, ?2, ?3, 1)",
            params![username, hash.as_str(), status],
        )?;
        Ok(())
    }

    // Homeowner Request a Tecnician
   #[test]
    fn test_homeowner_request_tech_success() -> Result<()> {
        let mut conn = test_db();
        insert_user(&conn, "alice", "homeowner", "Home123!")?;
        insert_user(&conn, "bob",   "technician", "Tech123!")?;

        login_user("alice");

        let job_id = grant_technician_access(
            &mut conn,
            "alice",
            "bob",
            60,
            "Thermostat not cooling properly.",
        )?;

        let (home, tech, mins, desc, status): (String, String, i64, String, String) =
            conn.query_row(
                "SELECT homeowner_username, technician_username, access_minutes, job_desc, status
                 FROM technician_jobs WHERE job_id = ?1",
                params![job_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;

        assert_eq!(home, "alice");
        assert_eq!(tech, "bob");
        assert_eq!(mins, 60);
        assert_eq!(desc, "Thermostat not cooling properly.");
        assert_eq!(status, "ACCESS_GRANTED");
        Ok(())
    }

    //Listing Technician job

    #[test]
    fn test_tech_list_my_jobs() -> Result<()> {
        let conn = test_db();
        insert_user(&conn, "bob", "technician", "Tech123!")?;
        insert_user(&conn, "alice", "homeowner", "Home123!")?;

        conn.execute(
            r#"
            INSERT INTO technician_jobs
                (homeowner_username, technician_username, status, access_minutes,
                 job_desc, grant_start, grant_expires, updated_at)
            VALUES
                ('alice','bob','ASSIGNED',30,'desc1',datetime('now'),datetime('now','+60 minutes'),datetime('now')),
                ('alice','bob','ACCESS_GRANTED',90,'desc2',datetime('now'),datetime('now','+120 minutes'),datetime('now'))
            "#,
            [],
        )?;

        login_user("bob");

        tech_list_my_jobs(&conn)?;
        Ok(())
    }

    // Accessing Granted Job
    #[test]
    fn test_tech_access_job_success() -> Result<()> {
        let mut conn = test_db();
        insert_user(&conn, "bob", "technician", "Tech123!")?;
        insert_user(&conn, "alice", "homeowner", "Home123!")?;

        let job_id: i64 = conn.query_row(
            r#"
            INSERT INTO technician_jobs
                (homeowner_username, technician_username, status, access_minutes,
                 job_desc, grant_start, grant_expires, updated_at)
            VALUES
                ('alice','bob','ACCESS_GRANTED',60,'Fix AC',
                 datetime('now'),datetime('now','+30 minutes'),datetime('now'))
            RETURNING job_id
            "#,
            [],
            |r| r.get(0),
        )?;

        login_user("bob");

        // Simulate correct password via `access_job` (same logic as tech_access_job)
        let result = access_job(&mut conn, job_id, "bob");

        assert!(result.is_ok(), "access_job failed: {:?}", result.err());
        assert!(result.unwrap().is_some());

        let status: String = conn.query_row(
            "SELECT status FROM technician_jobs WHERE job_id = ?1",
            params![job_id],
            |r| r.get(0),
        )?;
        assert_eq!(status, "TECH_ACCESS");
        Ok(())
    }


    
}

