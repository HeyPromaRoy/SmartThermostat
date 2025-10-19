use rusqlite::Connection;
use anyhow::Result;
use std::io::{self, Write};
use std::time::Duration;

use crate::ui;
use crate::auth;
use crate::logger;
use crate::db;
use crate::senser;

use senser::{run_dashboard_inline, Thresholds};

/// ===============================================================
/// MAIN MENU DISPATCHER
/// ===============================================================
pub fn main_menu(conn: &mut Connection, username: &str, role: &str) -> Result<()> {
    loop {
        match role {
            "homeowner" => {
                ui::homeowner_ui();
                if !homeowner_menu(conn, username, role)? {
                    break;
                }
            }
            "technician" => {
                ui::technician_ui();
                if !technician_menu(conn, username, role)? {
                    break;
                }
            }
            "guest" => {
                ui::guest_ui();
                if !guest_menu(conn, username, role)? {
                    break;
                }
            }
            "admin" => {
                ui::admin_ui();
                if !admin_menu(conn, username, role)? {
                    break;
                }
            }
            _ => {
                println!("⚠️ Unknown role: '{role}'. Please contact an administrator.");
                break;
            }
        }
    }
    Ok(())
}

//
// ========================== HOMEOWNER MENU ==========================
//
fn homeowner_menu(conn: &mut Connection, username: &str, role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => {
                println!("🏠 Registering a guest account...");
                // homeowners can only create guests (enforced inside register_user)
                auth::register_user(conn, Some((username, role)))?;
            }
            "2" => {db::list_guests_of_homeowner(conn, username)?;}
            "3" => println!("⚙️ Manage guests (coming soon)..."),
            "4" => println!("👤 Viewing your profile (coming soon)..."),
            "5" => {
                println!("Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("⚠️ Invalid choice, please try again.\n"),
        },
        None => {
            println!("End of input detected. Exiting...");
            return Ok(false);
        }
    }
    Ok(true)
}

//
// ========================== ADMIN MENU ==========================
//
fn admin_menu(conn: &mut Connection, username: &str, role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => {
                println!("👥 Registering a new user...");
                auth::register_user(conn, Some((username, role)))?;
            }
            "2" => println!("📋 Viewing all users (coming soon)..."),
            "3" => println!("⚙️ Managing users (coming soon)..."),
            "4" => println!("👤 Viewing admin profile (coming soon)..."),
            "6" => println!("📜 Viewing login logs (coming soon)..."),
            "7" => {
                println!("🧹 Clearing user lockouts...");
                logger::clear_lockout(conn, "admin", username)?;
            },
            "8" => {
                println!("🌡 Checking indoor temperature");
                if let Err(e) = senser:: run_dashboard_inline (Thresholds:: default(), Duration:: from_secs (1), Some (10)) {
                    eprintln! ("dashboard error: {e}");
                }
            }
            "5" | "10" => {
                println!("🔒 Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("⚠️ Invalid choice, please try again.\n"),
        },
        None => {
            println!("End of input detected. Exiting...");
            return Ok(false);
        }
    }
    Ok(true)
}

//
// ========================== TECHNICIAN MENU ==========================
//
fn technician_menu(conn: &mut Connection, username: &str, role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => {
                println!("🧰 Registering a guest account...");
                // Technicians can only create guests (enforced in auth.rs)
                auth::register_user(conn, Some((username, role)))?;
            }
            "2" => println!("📋 View guest(s) (coming soon)..."),
            "3" => println!("⚙️ Manage guests (coming soon)..."),
            "4" => println!("👤 Viewing your profile (coming soon)..."),
            "6" => println!("🧪 Running diagnostics (coming soon)..."),
            "7" => println!("📈 Testing sensor data (coming soon)..."),
            "8" => println!("🗒 Viewing system events (coming soon)..."),
            "5" | "10" => {
                println!("🔒 Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("⚠️ Invalid choice, please try again.\n"),
        },
        None => {
            println!("End of input detected. Exiting...");
            return Ok(false);
        }
    }
    Ok(true)
}

//
// ========================== GUEST MENU ==========================
//
fn guest_menu(_conn: &mut Connection, username: &str, _role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => println!("👤 Viewing guest profile for {}...", username),
            "2" => {
                println!("🌡 Checking indoor temperature");
                if let Err(e) = senser:: run_dashboard_inline (Thresholds:: default(), Duration:: from_secs (1), Some (10)) {
                    eprintln! ("dashboard error: {e}");
                }
            },
            "3" => println!("🌦 Retrieving outdoor weather stats (coming soon)..."),
            "4" => println!("❄️ Adjusting HVAC control (coming soon)..."),
            "5" | "10" => {
                println!("🔒 Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("⚠️ Invalid choice, please try again.\n"),
        },
        None => {
            println!("End of input detected. Exiting...");
            return Ok(false);
        }
    }
    Ok(true)
}

//
// ========================== SHARED HELPERS ==========================
//
pub fn prompt_input() -> Option<String> {
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
