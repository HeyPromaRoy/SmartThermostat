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
use crate::hvac;
use crate::weather;
use crate::function::{prompt_input, wait_for_enter};

use crate::profile::{HVACProfile, apply_profile};
use crate::hvac::HVACSystem;
use chrono::Local;

// ===============================================================
//                         PROFILE SELECTION MENU
// ===============================================================
fn profile_selection_menu(conn: &mut Connection, _username: &str) -> Result<()> {
    ui::profile_selection_ui();
    
    match prompt_input() {
        Some(choice) => {
            let profile = match choice.trim() {
                "1" => HVACProfile::Day,
                "2" => HVACProfile::Night,
                "3" => HVACProfile::Sleep,
                "4" => HVACProfile::Party,
                "5" => HVACProfile::Vacation,
                "6" => HVACProfile::Away,
                "0" => {
                    println!("Profile selection cancelled.");
                    return Ok(());
                }
                _ => {
                    println!("Invalid option.");
                    return Ok(());
                }
            };

            let mut hvac = HVACSystem::new();
            apply_profile(conn, &mut hvac, profile);
            println!("\n✓ Profile applied successfully!");
            wait_for_enter();
        }
        None => {
            println!("No input detected.");
        }
    }
    Ok(())
}

// ===============================================================
//                         MAIN MENU
// ===============================================================
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
                println!("Unknown role: '{role}'. Please contact an administrator.");
                break;
            }
        }
    }
    Ok(())
}

// ===============================================================
//                         HOMEOWNER MENU
// ===============================================================
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
            "1" => { 
                db::show_own_profile(conn, username)?;
                wait_for_enter();
            },
            "2" => {
                println!("Registering a guest account...");
                // homeowners can only create guests (enforced inside register_user)
                auth::register_user(conn, Some((username, role)))?;
            }
            "3" => {db::list_guests_of_homeowner(conn, username)?;}
            "4" => {
                guest::manage_guests_menu(conn, homeowner_id, username)?;},
            "5" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(senser::Thresholds::default()) {
                    eprintln!("dashboard error: {e}");
                }
                wait_for_enter();
            },
            "6" => {
                hvac_control_menu(conn, username)?;
            },
            "7" => {
                println!("Retrieving outdoor weather status...");
                if let Err(e) = weather::get_current_weather() {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "8" => {
                show_system_status(conn)?;
            },
            "9" => {
                println!("Profile settings (coming soon)...");
                wait_for_enter();
            },

            "0" => {
                println!("Logging out...");
                auth::logout_user(conn)?;
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

// ===============================================================
//                         ADMIN MENU
// ===============================================================
fn admin_menu(conn: &mut Connection, username: &str, role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => { 
                db::show_own_profile(conn, username)?;
                wait_for_enter();
            },
            "2" => {
                println!("Registering a new user...");
                auth::register_user(conn, Some((username, role)))?;
            }
            "3" => {
                println!("Viewing all users...");
                db::view_all_users(conn, role)?;
                wait_for_enter();
                },
            "4" => {
                println!("Managing users...");
                db::manage_user_status(conn, username, role)?;
            },
            
            "5" => {
                println!("Viewing security logs...");
                logger::view_security_log(conn, username, role)?;
                wait_for_enter();
            }
            "6" => {
                println!("Checking current lockouts...");
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
            }
            "7" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(senser::Thresholds::default()) {
                    eprintln!("dashboard error: {e}");
                }
                wait_for_enter();
            },
            "8" => {
                println!("Outdoor weather data...");
                if let Err(e) = weather::get_current_weather() {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "9" => {
                manage_profiles_menu(conn, username, role)?;
            },
            "0" => {
                println!("🔒 Logging out...");
                auth::logout_user(conn)?;
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

// ===============================================================
//                         TECHNICIAN MENU
// ===============================================================
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
            "1" => { 
                db::show_own_profile(conn, username)?;
                wait_for_enter();
            },
            "2" => {
                println!("Registering a guest account...");
                // Technicians can only create guests (enforced in auth.rs)
                auth::register_user(conn, Some((username, role)))?;
            }
            "3" => println!("View guest(s) (coming soon)..."),
            "4" => {guest::manage_guests_menu(conn, technician_id, username)?;},
            "5" => println!("Running diagnostics (coming soon)..."),
            "6" => println!("Viewing system events (coming soon)..."),
            "7"  => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(senser::Thresholds::default()) {
                    eprintln!("dashboard error: {e}");
                }
                wait_for_enter();
            },
            "8" => {
                println!("Outdoor weather data...");
                if let Err(e) = weather::get_current_weather() {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "9" => {
                profile_selection_menu(conn, username)?;
            },
            "0" => {
                println!("Logging out...");
                auth::logout_user(conn)?;
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

// ===============================================================
//                         HVAC CONTROL MENU
// ===============================================================
fn hvac_control_menu(conn: &mut Connection, username: &str) -> Result<()> {
    let mut hvac = hvac::HVACSystem::new();
    
    loop {
        ui::hvac_control_ui();

        match prompt_input() {
            Some(choice) => match choice.trim() {
                "1" => {
                    println!("\n⚙️  Temperature must be between 16°C and 40°C");
                    print!("Enter target temperature (°C): ");
                    io::stdout().flush()?;
                    if let Some(temp_str) = prompt_input() {
                        if let Ok(temp) = temp_str.trim().parse::<f32>() {
                            if hvac::HVACSystem::is_valid_temperature(temp) {
                                hvac.set_target_temperature(conn, temp);
                                println!("✓ Temperature set to {:.1}°C", temp);
                            } else {
                                println!("❌ Invalid temperature! Must be between 16°C and 40°C");
                            }
                        } else {
                            println!("❌ Invalid temperature value");
                        }
                    }
                }
                "2" => {
                    println!("\nSelect mode:");
                    println!("[1] Heat  [2] Cool  [3] Auto  [4] Fan  [5] Off");
                    if let Some(mode) = prompt_input() {
                        let new_mode = match mode.trim() {
                            "1" => hvac::HVACMode::Heating,
                            "2" => hvac::HVACMode::Cooling,
                            "3" => hvac::HVACMode::Auto,
                            "4" => hvac::HVACMode::FanOnly,
                            "5" => hvac::HVACMode::Off,
                            _ => {
                                println!("Invalid mode selection");
                                continue;
                            }
                        };
                        hvac.set_mode(conn, new_mode);
                        println!("Mode set to {:?}", new_mode);
                    }
                }
                "3" => {
                    hvac.update(conn);
                    wait_for_enter();
                }
                "4" => {
                    hvac.diagnostics(conn);
                    wait_for_enter();
                }
                "5" => {
                    profile_selection_menu(conn, username)?;
                }
                "6" => break,
                _ => println!("Invalid option. Please try again."),
            },
            None => break,
        }
    }
    Ok(())
}

// ===============================================================
//                  SYSTEM STATUS (TIME + SCHEDULE)
// ===============================================================
fn show_system_status(conn: &mut Connection) -> Result<()> {
    let now = Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let scheduled = crate::profile::current_scheduled_profile();
    println!("\n===== System Status =====");
    println!("Current time: {}", time_str);
    println!("Scheduled profile window: {:?}", scheduled);
    println!("Apply this scheduled profile now? (y/n)");
    if let Some(ans) = prompt_input() {
        if ans.trim().eq_ignore_ascii_case("y") {
            let mut hvac = HVACSystem::new();
            apply_profile(conn, &mut hvac, scheduled);
        }
    }
    wait_for_enter();
    Ok(())
}

// ===============================================================
//                  ADMIN: MANAGE PROFILE SETTINGS
// ===============================================================
fn manage_profiles_menu(conn: &mut Connection, admin_username: &str, current_role: &str) -> Result<()> {
    if current_role != "admin" { println!("Access denied: Only admins can manage profiles."); return Ok(()); }

    loop {
        println!("\n===== Profile Settings =====");
        let profiles = db::list_profile_rows(conn)?;
        for (idx, p) in profiles.iter().enumerate() {
            println!("[{}] {:<9}  mode={:<7} temp={:.1}°C", idx + 1, p.name, p.mode, p.target_temp);
        }
        println!("[E] Edit a profile  [R] Reset to defaults  [Q] Back");
        print!("Select option: "); io::stdout().flush().ok();
        let choice = prompt_input();
        let Some(choice) = choice else { break };
        let choice = choice.trim();
        if choice.eq_ignore_ascii_case("q") { break; }
        if choice.eq_ignore_ascii_case("e") {
            print!("Enter profile name (Day/Night/Sleep/Party/Vacation/Away): "); io::stdout().flush().ok();
            let name = match prompt_input() { Some(s) => s.trim().to_string(), None => continue };
            let valid = ["Day","Night","Sleep","Party","Vacation","Away"]; if !valid.contains(&name.as_str()) { println!("Invalid name."); continue; }
            // Mode selection
            println!("Select mode: [1] Off [2] Heating [3] Cooling [4] FanOnly [5] Auto");
            let mode_s = match prompt_input() { Some(s) => s.trim().to_string(), None => continue };
            let mode_str = match mode_s.as_str() { "1"=>"Off", "2"=>"Heating", "3"=>"Cooling", "4"=>"FanOnly", "5"=>"Auto", _=>{ println!("Invalid mode."); continue } };
            // Temperature
            print!("Enter target temperature (16-40 °C): "); io::stdout().flush().ok();
            let temp = match prompt_input().and_then(|s| s.trim().parse::<f32>().ok()) { Some(t)=>t, None=>{ println!("Invalid temp."); continue } };
            if !HVACSystem::is_valid_temperature(temp) { println!("❌ Invalid temperature! Must be between 16°C and 40°C"); continue; }
            // Optional greeting
            print!("Custom greeting (optional, Enter to skip): "); io::stdout().flush().ok();
            let greeting = prompt_input().map(|s| { let t = s.trim().to_string(); if t.is_empty(){ None } else { Some(t) } }).flatten();
            // Optional description
            print!("Description (optional, Enter to skip): "); io::stdout().flush().ok();
            let description = prompt_input().map(|s| { let t = s.trim().to_string(); if t.is_empty(){ None } else { Some(t) } }).flatten();

            db::update_profile_row(conn, &name, mode_str, temp, greeting.as_deref(), description.as_deref())?;
            let desc = format!("Profile '{}' updated: mode={}, temp={:.1}", name, mode_str, temp);
            logger::log_event(conn, admin_username, None, "HVAC", Some(&desc))?;
            println!("✓ Saved.");
        } else if choice.eq_ignore_ascii_case("r") {
            print!("Enter profile name to reset (or 'all'): "); io::stdout().flush().ok();
            let target = match prompt_input() { Some(s) => s.trim().to_string(), None => continue };
            if target.eq_ignore_ascii_case("all") {
                for nm in ["Day","Night","Sleep","Party","Vacation","Away"].iter() {
                    db::reset_profile_to_default(conn, nm)?;
                }
                logger::log_event(conn, admin_username, None, "HVAC", Some("All profiles reset to defaults"))?;
                println!("All profiles reset.");
            } else {
                db::reset_profile_to_default(conn, &target)?;
                let msg = format!("Profile '{}' reset to defaults", target);
                logger::log_event(conn, admin_username, None, "HVAC", Some(&msg))?;
                println!("{}", msg);
            }
        } else if let Ok(idx) = choice.parse::<usize>() {
            // quick apply selected profile view -> open editor-like flow
            if idx >= 1 && idx <= profiles.len() {
                let name = profiles[idx-1].name.clone();
                println!("Selected {}. Choose action: [A] Apply now  [E] Edit  [Back: Enter]", name);
                if let Some(act) = prompt_input() {
                    let t = act.trim().to_string();
                    if t.eq_ignore_ascii_case("a") {
                        let mut hvac = HVACSystem::new();
                        let prof = match name.as_str() { "Day"=>HVACProfile::Day, "Night"=>HVACProfile::Night, "Sleep"=>HVACProfile::Sleep, "Party"=>HVACProfile::Party, "Vacation"=>HVACProfile::Vacation, _=>HVACProfile::Away };
                        apply_profile(conn, &mut hvac, prof);
                    } else if t.eq_ignore_ascii_case("e") {
                        // loop back into edit branch by simulating 'E'
                        // Simpler: prompt again with E
                        println!("Re-open menu and choose [E] to edit.");
                    }
                }
            }
        } else {
            println!("Invalid option.");
        }
    }

    Ok(())
}

// ===============================================================
//                         GUEST MENU
// ===============================================================
fn guest_menu(conn: &mut Connection, username: &str, _role: &str) -> Result<bool> {
    match prompt_input() {
        Some(choice) => match choice.trim() {
            "1" => { 
                db::show_own_profile(conn, username)?;
                wait_for_enter();},
            "2" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(senser::Thresholds::default()) {
                    eprintln!("dashboard error: {e}");
                }
                wait_for_enter();
            },
            "3" => {
            println!("Retrieving outdoor weather statu...");
                if let Err(e) = weather::get_current_weather() {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "4" => hvac_control_menu(conn, username)?,
            "5" => {
                profile_selection_menu(conn, username)?;
            },
            "0" => {
                println!("🔒 Logging out...");
                auth::logout_user(conn)?;
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
