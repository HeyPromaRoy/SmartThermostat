
use smart_thermostat::senser::*;
use smart_thermostat::weather::*;
use smart_thermostat::auth::*;
use rusqlite::{Connection, Result};

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
    let co = get_indoor_COlevel().unwrap();
    assert!(co >= 0.0 && co <= 1000.0);
}

//  Test Weather: API returns valid struct
#[test]
fn test_fetch_weather() {
    let w = fetch_weather().expect("weather fetch failed");

    // At least condition string must not be empty
    assert!(!w.condition.is_empty(), "Condition should not be empty");
}

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