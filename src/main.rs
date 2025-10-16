mod ui;
mod auth;
mod db;
mod weather;

use rusqlite::Connection;
use std::io::{self, Write};
use anyhow::Result;


fn main() -> Result<()> {
    // Initialize database
    let mut conn = Connection::open("users.db")
        .expect("Failed to open or create database file.");
    db::init_db(&conn).expect("Failed to initialize database schema.");

    //Display the front-page (logo and menu)
    ui::front_page_ui();

    //Main entry loop
    loop {
        match prompt_input() {
            Some(choice) => match choice.trim() {
                // USER LOGIN
                "1" => {
                    ui::user_login_ui();
                    match auth::login_user(&conn) {
                        Ok(Some((username, role))) => {
                            main_menu(&conn, &username, &role)?;
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("Login error: {e}"),
                    }
                }

                // Weather data
                "2" => {
                println!("Fetching latest weather data...");
                let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

                 match rt.block_on(weather::fetch_weather()) {
                Ok(rec) => {
                // Store it safely
                 if let Err(e) = weather::store_weather(&rec) {
                eprintln!("Failed to save weather data: {e}");
                }

            // Present the latest weather data nicely
            println!("\n================ WEATHER DASHBOARD ================");
            println!("🕓 {}", rec.time);
            println!(
                "🌡 Temperature: {:.1}°F / {:.1}°C",
                rec.temperature_f.unwrap_or(f64::NAN),
                rec.temperature_c.unwrap_or(f64::NAN)
            );
            println!(
                "💧 Dew Point: {:.1}°F / {:.1}°C",
                rec.dewpoint_f.unwrap_or(f64::NAN),
                rec.dewpoint_c.unwrap_or(f64::NAN)
            );
            println!(
                "💦 Humidity: {:.0}%",
                rec.humidity.unwrap_or(f64::NAN)
            );
            println!(
                "💨 Wind: {:.1} mph ({:.0}°)",
                rec.wind_speed_mph.unwrap_or(f64::NAN),
                rec.wind_direction_deg.unwrap_or(f64::NAN)
            );
            println!("🌥 Condition: {}", rec.condition);
            println!("===================================================");

            // Pause before returning
            println!("\nPress ENTER to return to the main menu...");
            let _ = std::io::stdin().read_line(&mut String::new());
            ui::front_page_ui();
        }
        Err(e) => {
            eprintln!("Failed to fetch weather data: {e}");
            println!("\nPress ENTER to return to the main menu...");
            let _ = std::io::stdin().read_line(&mut String::new());
            ui::front_page_ui();
                }
            }
        }


                // About us
                "3" => {
                ui::about_ui();
                println!("\nPress ENTER to return to the main menu...");
                let _ = std::io::stdin().read_line(&mut String::new());
                ui::front_page_ui();
            }

                //EXIT
                "4" => {
                    println!("Goodbye!");
                    break;
                }

                _ => println!("Invalid choice. Please enter 1–4.\n"),
            },
            None => {
                println!("End of input detected. Exiting...");
                break;
            }
        }
    }

    Ok(())
}

/// Main control menu shown after successful login
fn main_menu(_conn: &Connection, _username: &str, role: &str) -> Result<()> {
    loop {
        // Choose which UI to show based on role
        match role {
            "homeowner" => ui::homeowner_ui(),
            "technician" => ui::technician_ui(),
            "guest" => ui::guest_ui(),
            "admin" => ui::admin_ui(),
            _ => {
        println!("Unknown role: '{role}'. Please contact admin.");
        continue; // skip to next loop iteration
            }
        }

        match prompt_input() {
            Some(choice) => match choice.trim() {
                // Homeowner / Technician options (for now placeholders)
                "1" => println!("🏠 Register a new guest (coming soon)..."),
                "2" => println!("📋 View guest list (coming soon)..."),
                "3" => println!("⚙️ Manage guests (coming soon)..."),
                "4" => println!("👤 Showing your profile (coming soon)..."),
                "5" => {
                    println!("🔒 Logging out and returning to home screen...\n");
                    ui::front_page_ui();
                    break;
                }
                _ => println!("⚠️ Invalid choice. Please try again.\n"),
            },
            None => {
                println!("End of input detected. Exiting...");
                break;
            }
        }
    }

    Ok(())
}

/// Helper: prompt for input and return trimmed string.
/// Returns `None` on EOF or read failure.
pub fn prompt_input() -> Option<String> {
    use std::io::{self, Write};

    // make sure buffered output is flushed
    if io::stdout().flush().is_err() {
        eprintln!("⚠️ Error flushing stdout.");
        return None;
    }

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => None, // EOF
        Ok(_) => Some(input.trim().to_string()),
        Err(e) => {
            eprintln!("⚠️ Error reading input: {e}");
            None
        }
    }
}

