
use smart_thermostat::senser::*;
use smart_thermostat::weather::*;
use smart_thermostat::auth::*;
use smart_thermostat::hvac::*;
use rusqlite::{Connection, Result};

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
    let mut conn = Connection::open_in_memory()?;

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

mod tests {
    use super::*; // Import everything from the parent module (HVACSystem, HVACMode, etc.)

    /// Helper function to create an in-memory database for testing
    /// This avoids touching a real database.
    fn test_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    /// Test that the `new()` constructor initializes HVACSystem correctly
    #[test]
    fn test_new_hvac_system() {
        let hvac = HVACSystem::new();
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
        let mut hvac = HVACSystem::new();

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
        let mut hvac = HVACSystem::new();

        hvac.set_target_temperature(&conn, 25.0);
        // Temperature should be set exactly as requested
        assert_eq!(hvac.target_temperature, 25.0);
    }

    /// Test that temperatures below the minimum are adjusted correctly
    #[test]
    fn test_set_target_temperature_below_min() {
        let conn = test_db();
        let mut hvac = HVACSystem::new();

        hvac.set_target_temperature(&conn, 10.0);
        // Should automatically set to MIN_TEMPERATURE
        assert_eq!(hvac.target_temperature, MIN_TEMPERATURE);
    }

    /// Test that temperatures above the maximum are adjusted correctly
    #[test]
    fn test_set_target_temperature_above_max() {
        let conn = test_db();
        let mut hvac = HVACSystem::new();

        hvac.set_target_temperature(&conn, 50.0);
        // Should automatically set to MAX_TEMPERATURE
        assert_eq!(hvac.target_temperature, MAX_TEMPERATURE);
    }

    /// Test that diagnostics function runs without panicking
    #[test]
    fn test_diagnostics_runs() {
        let conn = test_db();
        let hvac = HVACSystem::new();
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

    /// Simple test to ensure update() behaves for Auto mode heating scenario
    #[test]
    fn test_update_auto_mode_heating() {
        let conn = test_db();
        let mut hvac = HVACSystem::new();
        hvac.mode = HVACMode::Auto;
        hvac.target_temperature = 25.0;

        // Pretend current temperature is 23°C (below target - 0.5)
        let current_temp = 23.0;
        if current_temp < hvac.target_temperature - 0.5 {
            hvac.update(&conn); // Should trigger "Auto heating..."
        }
        // This test ensures update() can run without panicking in Auto mode
    }
}
