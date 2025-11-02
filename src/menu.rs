use rusqlite::{Connection, params};
use anyhow::Result;
use std::io::{self, Write};


use crate::{auth, db, guest, hvac, logger, senser, technician, ui, weather};
use crate::energy;
use crate::function::{prompt_input, wait_for_enter};

use crate::profile::{HVACProfile, apply_profile};
use crate::hvac::HVACSystem;
use chrono::Local;

// ===============================================================
//                    VACATION MODE CHECK
// ===============================================================
/// Check if vacation mode is currently active
fn is_vacation_mode_active(conn: &Connection) -> Result<bool> {
    if let Ok(Some(vacation_profile)) = db::get_profile_row(conn, "Vacation") {
        // Vacation mode is active if both start and end dates are set
        Ok(vacation_profile.vacation_start_date.is_some() && 
           vacation_profile.vacation_end_date.is_some())
    } else {
        Ok(false)
    }
}

// ===============================================================
//                         PROFILE SELECTION MENU
// ===============================================================
fn profile_selection_menu(conn: &mut Connection, username: &str, user_role: &str) -> Result<()> {
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

            // Special handling for Vacation profile
            if matches!(profile, HVACProfile::Vacation) {
                // Only homeowner can enable vacation mode
                if user_role != "homeowner" {
                    println!("❌ Access denied: Only homeowners can enable/disable vacation mode for security reasons.");
                    wait_for_enter();
                    return Ok(());
                }
                
                // Check if vacation mode is currently active
                let current_vacation = db::get_profile_row(conn, "Vacation")?;
                let is_vacation_active = current_vacation
                    .as_ref()
                    .and_then(|p| p.vacation_start_date.as_ref())
                    .is_some();
                
                // Require password verification for enabling/disabling vacation mode
                println!("\n🔐 Security Check: Please re-enter your password to modify vacation mode");
                print!("Password: ");
                io::stdout().flush()?;
                
                if let Some(password) = prompt_input() {
                    // Get stored password hash
                    let stored_hash: String = conn.query_row(
                        "SELECT hashed_password FROM users WHERE username = ?1",
                        params![username],
                        |row| row.get(0),
                    )?;
                    
                    if !auth::verify_password(&password, &stored_hash)? {
                        println!("❌ Incorrect password. Vacation mode change cancelled.");
                        wait_for_enter();
                        return Ok(());
                    }
                } else {
                    println!("❌ Password required. Vacation mode change cancelled.");
                    wait_for_enter();
                    return Ok(());
                }
                
                if is_vacation_active {
                    // Turning OFF vacation mode
                    println!("\n🏖️  Vacation mode is currently ACTIVE");
                    println!("Do you want to turn OFF vacation mode? (y/n): ");
                    if let Some(confirm) = prompt_input() {
                        if confirm.trim().eq_ignore_ascii_case("y") {
                            db::clear_vacation_dates(conn)?;
                            println!("✓ Vacation mode has been turned OFF.");
                            wait_for_enter();
                            return Ok(());
                        } else {
                            println!("Vacation mode remains active.");
                            wait_for_enter();
                            return Ok(());
                        }
                    }
                } else {
                    // Turning ON vacation mode - prompt for dates
                    println!("\n🏖️  Activating Vacation Mode");
                    println!("Please enter the vacation date range:");
                    
                    print!("Start date (mm-dd-yyyy): ");
                    io::stdout().flush()?;
                    let start_date = match prompt_input() {
                        Some(d) => d.trim().to_string(),
                        None => {
                            println!("❌ Date required. Vacation mode cancelled.");
                            wait_for_enter();
                            return Ok(());
                        }
                    };
                    
                    print!("End date (mm-dd-yyyy): ");
                    io::stdout().flush()?;
                    let end_date = match prompt_input() {
                        Some(d) => d.trim().to_string(),
                        None => {
                            println!("❌ Date required. Vacation mode cancelled.");
                            wait_for_enter();
                            return Ok(());
                        }
                    };
                    
                    // Validate date format (basic check)
                    if !validate_date_format(&start_date) || !validate_date_format(&end_date) {
                        println!("❌ Invalid date format. Please use mm-dd-yyyy format.");
                        wait_for_enter();
                        return Ok(());
                    }
                    
                    // Save vacation dates
                    db::set_vacation_dates(conn, &start_date, &end_date)?;
                    
                    let mut hvac = HVACSystem::new(conn);
                    apply_profile(conn, &mut hvac, profile, username, user_role);
                    println!("\n✓ Vacation mode activated from {} to {}!", start_date, end_date);
                    wait_for_enter();
                    return Ok(());
                }
            }

            let mut hvac = HVACSystem::new(conn);
            apply_profile(conn, &mut hvac, profile, username, user_role);
            println!("\n✓ Profile applied successfully!");
            wait_for_enter();
        }
        None => {
            println!("No input detected.");
        }
    }
    Ok(())
}

// Helper function to validate date format mm-dd-yyyy
fn validate_date_format(date_str: &str) -> bool {
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    
    // Check if parts are numeric and in valid ranges
    if let (Ok(month), Ok(day), Ok(year)) = (
        parts[0].parse::<u32>(),
        parts[1].parse::<u32>(),
        parts[2].parse::<u32>(),
    ) {
        month >= 1 && month <= 12 && day >= 1 && day <= 31 && year >= 2000 && year <= 2100
    } else {
        false
    }
}

// ===============================================================
//                         MAIN MENU
// ===============================================================
pub fn main_menu(conn: &mut Connection, username: &str, role: &str) -> Result<()> {
    // Check if vacation mode is active for guests and technicians
    if role == "guest" || role == "technician" {
        if is_vacation_mode_active(conn)? {
            println!("\n🏖️ ═══════════════════════════════════════════════════════");
            println!("   VACATION MODE ACTIVE - ACCESS RESTRICTED");
            println!("   ═══════════════════════════════════════════════════════");
            println!("\n   The homeowner has enabled vacation mode.");
            println!("   For security reasons, guest and technician access");
            println!("   is temporarily disabled while the property is vacant.");
            println!("\n   Please contact the homeowner for more information.");
            println!("\n🏖️ ═══════════════════════════════════════════════════════\n");
            
            // Log the blocked access attempt
            let _ = logger::log_event(
                conn,
                username,
                Some(username),
                "HVAC",
                Some(&format!("Access denied for {} due to active vacation mode", role)),
            );
            
            wait_for_enter();
            // Log out the user
            auth::logout_user(conn)?;
            return Ok(());
        }
    }
    
    loop {
        match role {
            "homeowner" => {
                ui::homeowner_ui();
                if !homeowner_menu(conn, username, role)? {
                    break;
                }
            }
            "guest" => {
                ui::guest_ui();
                if !guest_menu(conn, username, role)? {
                    break;
                }
            }
            "technician" => {
                ui::technician_ui();
                if !technician_menu(conn, username, role)? {
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
                guest::manage_guests_menu(conn, homeowner_id, username, role, username)?;},
            "5" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(senser::Thresholds::default()) {
                    eprintln!("dashboard error: {e}");
                }
                wait_for_enter();
            },
            "6" => {
                hvac_control_menu(conn, username, role)?;
            },
            "7" => {
                println!("Retrieving outdoor weather status...");
                if let Err(e) = weather::get_current_weather(conn) {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "8" => {
                show_system_status(conn, username, role)?;
            },
            "9" => {
                manage_profiles_menu(conn, username, role)?;
            },

            "A" => {
                if let Err(e) = energy::view_energy_usage(conn, username) {
                        println!("Error generating energy report: {}", e);
                    }
                    wait_for_enter();
            }

            "C" => {
                technician::homeowner_request_tech(conn)?;
                wait_for_enter();
            }

            "D" => {
                println!("Your active technician access grants:");
                db::list_active_grants(conn, username)?;
                wait_for_enter();
            }
            "B" => {

                if let Err(e) = energy::compare_energy_usage(conn, username) {
                        println!("Error comparing energy usage: {}", e);
                        wait_for_enter(); }
            }

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
    
    let _technician_id = match db::get_user_id_and_role(conn, username)? {
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
                if let Err(e) = technician::tech_list_my_jobs(conn) {
                eprintln!("Error: {e}");
            }
            wait_for_enter();}

            "3" => {    
            println!("Use a homeowner access grant…");
            if let Err(e) = technician::tech_access_job(conn) {
            eprintln!("Error: {e}");}
            },
            "4" => {    
                println!("Enter homeowner username to manage their guests:");
            if let Some(h_in) = prompt_input() {
            let homeowner_username = h_in.trim();
            if homeowner_username.is_empty() {
            println!("No homeowner entered.");
            } else {
            match db::get_user_id_and_role(conn, homeowner_username)? {
                Some((homeowner_id, h_role)) if h_role == "homeowner" => {
                    // Pass homeowner_id (not technician_id) and the homeowner's username
                    guest::manage_guests_menu(conn, homeowner_id,
                        username,            // acting technician username
                        role,                // "technician"
                        homeowner_username,  // target homeowner
                    )?;
                }
                _ => { println!("'{}' is not a valid homeowner.", homeowner_username);}
                }
            }
            } else { println!("Cancelled."); }
        }
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
                if let Err(e) = weather::get_current_weather(conn) {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "9" => {
                manage_profiles_menu(conn, username, role)?;
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
//                         GUEST MENU
// ===============================================================
fn guest_menu(conn: &mut Connection, username: &str, role: &str) -> Result<bool> {
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
                if let Err(e) = weather::get_current_weather(conn) {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "4" => hvac_control_menu(conn, username, role)?,
            "5" => {
                profile_selection_menu(conn, username, role)?;
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

// ===============================================================
//                         HVAC CONTROL MENU
// ===============================================================
fn hvac_control_menu(conn: &mut Connection, username: &str, user_role: &str) -> Result<()> {
    let mut hvac = hvac::HVACSystem::new(conn);
    
    loop {
        ui::hvac_control_ui(user_role);

        match prompt_input() {
            Some(choice) => match choice.trim() {
                "1" => {
                    println!("\n🌡️  Select HVAC Mode:");
                    println!("[1] 🔥 Heating  [2] ❄️  Cooling  [3] 🤖 Auto  [4] 💨 Fan Only  [5] ⭕ Off");
                    if let Some(mode) = prompt_input() {
                        let new_mode = match mode.trim() {
                            "1" => hvac::HVACMode::Heating,
                            "2" => hvac::HVACMode::Cooling,
                            "3" => hvac::HVACMode::Auto,
                            "4" => hvac::HVACMode::FanOnly,
                            "5" => hvac::HVACMode::Off,
                            _ => {
                                println!("❌ Invalid mode selection");
                                continue;
                            }
                        };
                        
                        let old_mode_str = format!("{:?}", hvac.mode);
                        let old_temp = hvac.target_temperature;
                        
                        // Set temperature for modes that need it (not Off or FanOnly)
                        if matches!(new_mode, hvac::HVACMode::Heating | hvac::HVACMode::Cooling | hvac::HVACMode::Auto) {
                            let (min_temp, max_temp) = new_mode.temperature_range();
                            println!("\n🌡️  Enter target temperature for {:?} mode ({:.0}-{:.0}°C):", new_mode, min_temp, max_temp);
                            print!("Temperature: ");
                            io::stdout().flush()?;
                            
                            if let Some(temp_str) = prompt_input() {
                                if let Ok(temp) = temp_str.trim().parse::<f32>() {
                                    if new_mode.is_valid_temperature_for_mode(temp) {
                                        hvac.set_mode(conn, new_mode);
                                        hvac.set_target_temperature(conn, temp);
                                        
                                        // Prompt for light status
                                        println!("\n💡 Light/Lamp: [1] ON  [2] OFF");
                                        print!("Choice: ");
                                        io::stdout().flush()?;
                                        if let Some(light_choice) = prompt_input() {
                                            let light_status = match light_choice.trim() {
                                                "1" => "ON",
                                                "2" => "OFF",
                                                _ => "OFF", // Default to OFF
                                            };
                                            hvac.set_light_status(conn, light_status);
                                        }
                                        
                                        // Log both changes
                                        let new_mode_str = format!("{:?}", new_mode);
                                        let _ = db::log_mode_changed(conn, username, user_role, &old_mode_str, &new_mode_str);
                                        let _ = db::log_temperature_changed(conn, username, user_role, old_temp, temp);
                                        
                                        println!("✅ Mode set to {:?} with target {:.1}°C, Light: {}", new_mode, temp, hvac.light_status);
                                    } else {
                                        println!("❌ Invalid temperature for {:?} mode! Must be between {:.0}°C and {:.0}°C", 
                                                 new_mode, min_temp, max_temp);
                                        continue;
                                    }
                                } else {
                                    println!("❌ Invalid temperature value");
                                    continue;
                                }
                            }
                        } else {
                            // Fan Only or Off - just set mode and light, no temperature needed
                            hvac.set_mode(conn, new_mode);
                            
                            // Prompt for light status
                            println!("\n💡 Light/Lamp: [1] ON  [2] OFF");
                            print!("Choice: ");
                            io::stdout().flush()?;
                            if let Some(light_choice) = prompt_input() {
                                let light_status = match light_choice.trim() {
                                    "1" => "ON",
                                    "2" => "OFF",
                                    _ => "OFF", // Default to OFF
                                };
                                hvac.set_light_status(conn, light_status);
                            }
                            
                            let new_mode_str = format!("{:?}", new_mode);
                            let _ = db::log_mode_changed(conn, username, user_role, &old_mode_str, &new_mode_str);
                            println!("✅ Mode set to {:?}, Light: {}", new_mode, hvac.light_status);
                        }
                    }
                }
                "2" => {
                    hvac.update(conn);
                    wait_for_enter();
                }
                "3" => {
                    hvac.diagnostics(conn);
                    wait_for_enter();
                }
                "4" => {
                    // For homeowners: Choose Profile
                    // For guests/technicians: Return to main menu
                    if user_role == "homeowner" {
                        profile_selection_menu(conn, username, user_role)?;
                    } else {
                        // Option 4 is "Return to Main Menu" for non-homeowners
                        break;
                    }
                }
                "5" => {
                    // Only homeowners see option 5 (Return to Main Menu)
                    if user_role == "homeowner" {
                        break;
                    } else {
                        println!("Invalid option. Please try again.");
                    }
                }
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
fn show_system_status(conn: &mut Connection, username: &str, user_role: &str) -> Result<()> {
    let now = Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let scheduled = crate::profile::current_scheduled_profile();
    println!("\n===== System Status =====");
    println!("Current time: {}", time_str);
    println!("Scheduled profile window: {:?}", scheduled);
    println!("Apply this scheduled profile now? (y/n)");
    if let Some(ans) = prompt_input() {
        if ans.trim().eq_ignore_ascii_case("y") {
            let mut hvac = HVACSystem::new(conn);
            apply_profile(conn, &mut hvac, scheduled, username, user_role);
        }
    }
    wait_for_enter();
    Ok(())
}

// ===============================================================
//                  ADMIN: MANAGE PROFILE SETTINGS
// ===============================================================
fn manage_profiles_menu(conn: &mut Connection, admin_username: &str, current_role: &str) -> Result<()> {
    if current_role != "homeowner" && current_role != "admin" && current_role != "technician" { 
        println!("Access denied: Only homeowners, technicians, and admins can manage profiles."); 
        return Ok(()); 
    }

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
            
            // Get old values for logging
            let old_profile = db::get_profile_row(conn, &name).ok().flatten();
            let old_mode = old_profile.as_ref().map(|p| p.mode.as_str());
            let old_temp = old_profile.as_ref().map(|p| p.target_temp);
            
            // No description editing per request
            db::update_profile_row(conn, &name, mode_str, temp, greeting.as_deref(), None)?;
            
            // Log profile edit to HVAC activity log
            let _ = db::log_profile_edited(conn, admin_username, current_role, &name, old_mode, mode_str, old_temp, temp);
            
            let desc = format!("Profile '{}' updated: mode={}, temp={:.1}", name, mode_str, temp);
            println!("✓ Saved (logged: {})", desc);
        } else if choice.eq_ignore_ascii_case("r") {
            print!("Enter profile name to reset (or 'all'): "); io::stdout().flush().ok();
            let target = match prompt_input() { Some(s) => s.trim().to_string(), None => continue };
            if target.eq_ignore_ascii_case("all") {
                for nm in ["Day","Night","Sleep","Party","Vacation","Away"].iter() {
                    db::reset_profile_to_default(conn, nm)?;
                    let _ = db::log_profile_reset(conn, admin_username, current_role, nm);
                }
                println!("All profiles reset (logged).");
            } else {
                db::reset_profile_to_default(conn, &target)?;
                let _ = db::log_profile_reset(conn, admin_username, current_role, &target);
                let msg = format!("Profile '{}' reset to defaults", target);
                println!("{} (logged)", msg);
            }
        } else if let Ok(idx) = choice.parse::<usize>() {
            // quick apply selected profile view -> open editor-like flow
            if idx >= 1 && idx <= profiles.len() {
                let name = profiles[idx-1].name.clone();
                println!("Selected {}. Choose action: [A] Apply now  [E] Edit  [Back: Enter]", name);
                if let Some(act) = prompt_input() {
                    let t = act.trim().to_string();
                    if t.eq_ignore_ascii_case("a") {
                        let mut hvac = HVACSystem::new(conn);
                        let prof = match name.as_str() { "Day"=>HVACProfile::Day, "Night"=>HVACProfile::Night, "Sleep"=>HVACProfile::Sleep, "Party"=>HVACProfile::Party, "Vacation"=>HVACProfile::Vacation, _=>HVACProfile::Away };
                        apply_profile(conn, &mut hvac, prof, admin_username, current_role);
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



