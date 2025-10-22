use rusqlite::Connection;
use anyhow::Result;
use std::io::{self, Write};
use std::time::Duration;

use crate::ui;
use crate::auth;
use crate::logger;
use crate::db;
use crate::guest;
use crate::senser;

use senser::{run_dashboard_inline, Thresholds};

/// ===============================================================
/// MAIN MENU DISPATCHER
/// ===============================================================
pub fn main_menu(conn: &mut Connection, logger_conn: &Connection, username: &str, role: &str) -> Result<()> {
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
                if !admin_menu(conn, logger_conn, username, role)? {
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
    let homeowner_id = match db::get_user_id_and_role(conn, username)? {
    Some((id, role)) if role == "homeowner" => id,
    _ => {
        println!("Access denied: only homeowners can manage guests.");
        return Ok(false);
        }
    };
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => db::show_own_profile(conn, username)?,
            "2" => {
                println!("Registering a guest account...");
                // homeowners can only create guests (enforced inside register_user)
                auth::register_user(conn, Some((username, role)))?;
            }
            "3" => {db::list_guests_of_homeowner(conn, username)?;}
            "4" => {
                manage_guests_menu(conn, homeowner_id, username)?;},
            "5" => {
                println!("Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("Invalid choice, please try again.\n"),
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
fn admin_menu(conn: &mut Connection, logger_conn: &Connection, username: &str, role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => db::show_own_profile(conn, username)? ,
            "2" => {
                println!("Registering a new user...");
                auth::register_user(conn, Some((username, role)))?;
            }
            "3" => {
                println!("📋 Viewing all users...");
                db::view_all_users(conn, role)?;},
            "4" => {
                println!("⚙️ Managing users...");
                db::manage_user_status(conn, username, role)?;
            },
            
            "6" => {
                println!("📜 Viewing security logs...");
                logger::view_security_log(logger_conn, username, role)?;
            },
            "7" => {
                println!("🧹 Checking current lockouts...");
                //show all locked accounts
                logger::clear_lockout(conn, "admin", None)?;

                // ask admin if they want to clear a specific user
                println!("\nEnter username to clear lockout (or press Enter to cancel): ");
                let target = prompt_input();

                if let Some(user_input) = target {
                    let trimmed = user_input.trim();
                    if !trimmed.is_empty() {
                        // Call again with Some(username)
                        logger::clear_lockout(conn, "admin", Some(trimmed))?;
                    } else {
                        println!("No username entered. Returning to menu.");
                    }
                } else {
                    println!("No input detected. Returning to menu.");
                }
            },
            "8" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(Thresholds::default(), Duration::from_secs(1), Some(10)) {
                    eprintln!("dashboard error: {e}");
                }
            },
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
    
    let technician_id = match db::get_user_id_and_role(conn, username)? {
        Some((id, role)) if role == "technician" => id,
        _ => {
            println!("Access denied: invalid technician account.");
            return Ok(false);
        }
    };

    
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => db::show_own_profile(conn, username)?,
            "2" => {
                println!("Registering a guest account...");
                // Technicians can only create guests (enforced in auth.rs)
                auth::register_user(conn, Some((username, role)))?;
            }
            "3" => println!("View guest(s) (coming soon)..."),
            "4" => {manage_guests_menu(conn, technician_id, username)?;},
            
            "6" => println!("Running diagnostics (coming soon)..."),
            "7" => println!("Testing sensor data (coming soon)..."),
            "8" => println!("Viewing system events (coming soon)..."),
            "5" | "10" => {
                println!("Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("Invalid choice, please try again.\n"),
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
fn guest_menu(conn: &mut Connection, username: &str, _role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => db::show_own_profile(conn, username)?,
            "2" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(Thresholds::default(), Duration::from_secs(1), Some(10)) {
                    eprintln!("dashboard error: {e}");
                }
            },
            "3" => println!("Retrieving outdoor weather stats (coming soon)..."),
            "4" => println!("Adjusting HVAC control (coming soon)..."),
            "5" | "10" => {
                println!("🔒 Logging out...");
                ui::front_page_ui();
                return Ok(false);
            }
            _ => println!("Invalid choice, please try again.\n"),
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
        eprintln!("Error flushing stdout.");
        return None;
    }

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(0) => None, // EOF
        Ok(_) => Some(input.trim().to_string()),
        Err(e) => {
            eprintln!("Error reading input: {e}");
            None
        }
    }
}

fn manage_guests_menu(conn: &mut Connection, owner_id: i64, homeowner_username: &str) -> Result<()> {
    loop {
        ui::manage_guest_menu();

        let mut choice = String::new();
        std::io::stdin().read_line(&mut choice)?;
        let choice = choice.trim();

        match choice {
            "1" => {
                println!("\n======= Reset Guest PIN =======");
                if let Err(e) = guest::reset_guest_pin(conn, owner_id, homeowner_username) {
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
                        if let Err(e) = guest::enable_guest(conn, owner_id, homeowner_username) {
                            println!("Error: {}", e);
                        }
                    }
                    "2" => {
                        if let Err(e) = guest::disable_guest(conn, owner_id, homeowner_username) {
                            println!("Error: {}", e);
                        }
                    }
                    _ => println!("Invalid sub-option."),
                }
            }
            "3" => {println!("\n======= Delete Guest =======");
        if let Err(e) = guest::delete_guest(conn, owner_id, homeowner_username) {
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


