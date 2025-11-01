
use smart_thermostat::senser::*;
use smart_thermostat::weather::*;
use smart_thermostat::auth::*;
use smart_thermostat::hvac::*;
use smart_thermostat::logger::*;
use anyhow::Result;
use rusqlite::{Connection};



mod tests {
    use super::*;


// ---- Test Senser.rs  ----

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

// ---- Test Weather.rs  ----

//  Test Weather: API returns valid struct
#[test]
fn test_fetch_weather() {
    let w = fetch_weather().expect("weather fetch failed");

    // At least condition string must not be empty
    assert!(!w.condition.is_empty(), "Condition should not be empty");
}

// ---- Test auth.rs ---- 

#[test]
fn test_register_and_login() -> Result<()> {
    // Create an in-memory SQLite database for testing
    let  mut conn = Connection::open_in_memory()?;

    // Initialize a simple users table
    conn.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            hashed_password TEXT NOT NULL,
            user_status TEXT NOT NULL,
            is_active INTEGER DEFAULT 1,
            creator TEXT,
            homeowner_id INTEGER
        )",
        [],
    )?;

    // Create a fake acting admin user directly in DB
    let hashed = hash_password("Admin123!").unwrap();
    conn.execute(
        "INSERT INTO users (username, hashed_password, user_status, is_active)
         VALUES (?1, ?2, ?3, 1)",
        [&"admin", &hashed.as_str(), &"admin"],
    )?;

    // Use register_user function to add a new technician
    let acting_user = Some(("admin", "admin"));
    
    // Since register_user interacts with CLI, we can't fully automate password input here
    // We'll just check that the function signature compiles and can be called.
    // A real test would mock stdin for read_password()
    
    // Ensure function can be called (integration test compilation check)
    let _result = register_user(&mut conn, acting_user);

    Ok(())
}

#[test]
fn test_username_validation() {
    assert!(username_is_valid("user_123"));
    assert!(!username_is_valid("bad user"));
    assert!(!username_is_valid("x")); // too short
    assert!(!username_is_valid("this_is_way_too_long_for_a_username_1234567890"));
}

#[test]
fn test_password_strength() {
    assert!(password_is_strong("StrongPass1!", "user"));
    assert!(!password_is_strong("weak", "user"));        // too short
    assert!(!password_is_strong("password123", "user")); // missing special char
    assert!(!password_is_strong("User123!", "user"));    // contains username
}

// ---- Test hvac.rs ----


    /// Helper function to create an in-memory database for testing
    /// This avoids touching a real database.
    fn test_db() -> Connection {
       let  conn = Connection::open_in_memory().unwrap();

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
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE lockouts (
                username TEXT PRIMARY KEY COLLATE NOCASE,
                locked_until TEXT NOT NULL,
                lock_count INTEGER DEFAULT 1
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE users (
                username TEXT PRIMARY KEY COLLATE NOCASE,
                last_login_time TEXT,
                updated_at TEXT
            )",
            [],
        )
        .unwrap();

        conn
    }

    
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

    /// Test the temperature validation function
    #[test]
    fn test_is_valid_temperature() {
        // Values within limits should return true
        assert!(HVACSystem::is_valid_temperature(16.0));
        assert!(HVACSystem::is_valid_temperature(22.5));
        assert!(HVACSystem::is_valid_temperature(40.0));
        // Values outside limits should return false
        assert!(!HVACSystem::is_valid_temperature(15.9));
        assert!(!HVACSystem::is_valid_temperature(40.1));
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

// ---- Test logger.rs ----


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

// Test: clear_lockout() — admin can clear specific user

#[test]
fn test_clear_lockout_admin_clears_user() -> Result<()> {
    let conn = test_db();

    // Add a locked-out user
    conn.execute(
        "INSERT INTO lockouts (username, locked_until, lock_count)
         VALUES ('bob', datetime('now', '+120 seconds'), 3)",
        [],
    )?;

    // Admin clears the lockout
    clear_lockout(&conn, "admin", Some("bob"))?;

    // Verify it's gone
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lockouts WHERE username='bob'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 0);

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

}

