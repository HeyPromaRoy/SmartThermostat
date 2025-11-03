use rusqlite::{Connection, params};
use anyhow::Result;
use std::io::{self, Write};


use crate::{auth, db, guest, hvac, logger, senser, technician, ui, weather, diagnostic};
use crate::energy;
use crate::function::{prompt_input, wait_for_enter};

use crate::profile::{HVACProfile, apply_profile};
use crate::hvac::{HVACSystem, HVACMode};
use chrono::Local;

// ===============================================================
//                    VACATION MODE CHECK
// ===============================================================
// Check if vacation mode is currently active
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
    // Load all profiles from database
    let profiles = db::list_profile_rows(conn)?;
    
    if profiles.is_empty() {
        println!("❌ No profiles available.");
        wait_for_enter();
        return Ok(());
    }
    
    ui::profile_selection_ui(&profiles);
    
    match prompt_input() {
        Some(choice) => {
            if choice.trim() == "0" {
                println!("Profile selection cancelled.");
                return Ok(());
            }
            
            // Parse the selection
            let selection: usize = match choice.trim().parse() {
                Ok(num) if num > 0 && num <= profiles.len() => num,
                _ => {
                    println!("Invalid option.");
                    return Ok(());
                }
            };
            
            // Get the selected profile
            let selected_profile = &profiles[selection - 1];
            let profile_name = &selected_profile.name;
            
            // Check if it's a default profile or custom
            let profile_opt = match profile_name.as_str() {
                "Day" => Some(HVACProfile::Day),
                "Night" => Some(HVACProfile::Night),
                "Sleep" => Some(HVACProfile::Sleep),
                "Party" => Some(HVACProfile::Party),
                "Vacation" => Some(HVACProfile::Vacation),
                "Away" => Some(HVACProfile::Away),
                _ => None, // Custom profile
            };
            
            // For custom profiles, handle them separately
            if profile_opt.is_none() {
                // This is a custom profile - apply it directly from database
                apply_custom_profile(conn, username, user_role, selected_profile)?;
                println!("\n✓ Profile '{}' applied successfully!", profile_name);
                wait_for_enter();
                return Ok(());
            }
            
            let profile = profile_opt.unwrap();

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

            // Check if vacation mode is currently active and switching to a different profile
            if !matches!(profile, HVACProfile::Vacation) && is_vacation_mode_active(conn)? {
                // Only homeowner can deactivate vacation mode
                if user_role != "homeowner" {
                    println!("❌ Access denied: Vacation mode is active. Only homeowners can change profiles.");
                    wait_for_enter();
                    return Ok(());
                }
                
                println!("\n⚠️  VACATION MODE IS CURRENTLY ACTIVE");
                println!("════════════════════════════════════════════════");
                println!("You are attempting to switch to a different profile.");
                println!("This will deactivate vacation mode and restore");
                println!("guest and technician access to the system.");
                println!("════════════════════════════════════════════════");
                print!("\nDo you want to turn OFF vacation mode and switch to the new profile? (y/n): ");
                io::stdout().flush()?;
                
                if let Some(confirm) = prompt_input() {
                    if confirm.trim().eq_ignore_ascii_case("y") {
                        // Require password verification
                        println!("\n🔐 Security Check: Please re-enter your password to deactivate vacation mode");
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
                                println!("❌ Incorrect password. Profile change cancelled.");
                                wait_for_enter();
                                return Ok(());
                            }
                        } else {
                            println!("❌ Password required. Profile change cancelled.");
                            wait_for_enter();
                            return Ok(());
                        }
                        
                        // Clear vacation dates
                        db::clear_vacation_dates(conn)?;
                        println!("✓ Vacation mode has been deactivated.");
                        println!("✓ Guest and technician access is now restored.");
                    } else {
                        println!("Profile change cancelled. Vacation mode remains active.");
                        wait_for_enter();
                        return Ok(());
                    }
                } else {
                    println!("Profile change cancelled. Vacation mode remains active.");
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
    match db::get_user_id_and_role(conn, username)? {
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
            "2" => {guest::manage_guests_menu(conn, username, role, username)?;}
            "3" => {
                println!("🌡 Checking indoor temperature...");
                if let Err(e) = senser::run_dashboard_inline(senser::Thresholds::default()) {
                    eprintln!("dashboard error: {e}");
                }
                wait_for_enter();
            },

            "4" => {
                println!("Retrieving outdoor weather status...");
                if let Err(e) = weather::get_current_weather(conn) {
                    eprintln!("❌ Error: {:?}", e);
                }
                wait_for_enter();
            },
            "5" => {hvac_control_menu(conn, username, role)?;},
            "6" => {show_system_status(conn, username, role)?;},
            "7" => {manage_profiles_menu(conn, username, role)?;},
            "8" => {
                if let Err(e) = energy::view_energy_usage(conn, username) {
                        println!("Error generating energy report: {}", e);
                    }
                    wait_for_enter();
            },
            "9" => { 
                if let Err(e) = energy::compare_energy_usage(conn, username) {
                println!("Error comparing energy usage: {}", e);
                wait_for_enter(); }
                technician::homeowner_request_tech(conn)?;
                wait_for_enter();
            },
            "A" => {
                technician::homeowner_request_tech(conn)?;
                wait_for_enter();
            }
            "B" => {
                println!("Your active technician access grants:");
                db::list_active_grants(conn, username)?;
                wait_for_enter();
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
    
 match db::get_user_id_and_role(conn, username)? {
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
                Some((_homeowner_id, h_role)) if h_role == "homeowner" => {
                    // Pass homeowner_id (not technician_id) and the homeowner's username
                    guest::manage_guests_menu(conn,
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
            "5" => {
                println!("Running diagnostics...");
                let _ = diagnostic::run_diagnostics();
                wait_for_enter();
            },
            "6" => {
                show_system_status(conn, username, role)?;
            },
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
                    // Option 3 behavior depends on user role
                    if user_role == "homeowner" {
                        // Homeowners: Choose Profile
                        profile_selection_menu(conn, username, user_role)?;
                        // Reload HVAC state from database after profile change
                        hvac = hvac::HVACSystem::new(conn);
                    } else if user_role == "guest" {
                        // Guests: Return to Main Menu (they have Choose Profile in main menu)
                        break;
                    } else {
                        // Technicians: Run Diagnostics
                        hvac.diagnostics(conn);
                        wait_for_enter();
                    }
                }
                "4" => {
                    // Option 4 is "Return to Main Menu" for homeowners and technicians
                    break;
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
    // Only homeowners and technicians can view system status
    if user_role != "homeowner" && user_role != "technician" {
        println!("Access denied: Only homeowners and technicians can view system status.");
        wait_for_enter();
        return Ok(());
    }
    
    let now = Local::now();
    let time_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let scheduled = crate::profile::current_scheduled_profile();
    
    println!("\n╔═══════════════════════════════════════════════════════╗");
    println!("║              SYSTEM STATUS & INFORMATION              ║");
    println!("╠═══════════════════════════════════════════════════════╣");
    println!("║ Current Time: {:<39} ║", time_str);
    println!("║ Scheduled Profile Window: {:<27} ║", format!("{:?}", scheduled));
    println!("╚═══════════════════════════════════════════════════════╝");
    
    // Display HVAC activity log
    db::view_hvac_activity_log(conn, username, user_role)?;
    
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
        println!("\n═══════════════════════════════════════════════════");
        println!("          🔧 PROFILE MANAGEMENT MENU");
        println!("═══════════════════════════════════════════════════");
        let profiles = db::list_profile_rows(conn)?;
        println!("\n📋 Existing Profiles:");
        for (idx, p) in profiles.iter().enumerate() {
            let profile_type = if db::is_default_profile(&p.name) { "🔒 Default" } else { "✨ Custom" };
            println!("[{}] {:<15} | {} | mode={:<8} | temp={:.1}°C | fan={:<6} | light={:<3}", 
                idx + 1, p.name, profile_type, p.mode, p.target_temp, p.fan_speed, p.light_status);
        }
        println!("\n📝 Options:");
        println!("[C] Create New Profile    [E] Edit Profile       [D] Delete Profile");
        println!("[R] Reset to Defaults     [Q] Back to Main Menu");
        print!("\nSelect option: "); io::stdout().flush().ok();
        let choice = prompt_input();
        let Some(choice) = choice else { break };
        let choice = choice.trim();
        
        if choice.eq_ignore_ascii_case("q") { 
            break; 
        } else if choice.eq_ignore_ascii_case("c") {
            // CREATE NEW PROFILE
            create_new_profile_flow(conn, admin_username, current_role)?;
        } else if choice.eq_ignore_ascii_case("d") {
            // DELETE PROFILE
            delete_profile_flow(conn, admin_username, current_role)?;
        } else if choice.eq_ignore_ascii_case("e") {
            // EDIT PROFILE (with full control)
            edit_profile_full_flow(conn, admin_username, current_role)?;
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
                let selected_profile = &profiles[idx-1];
                let name = selected_profile.name.clone();
                println!("Selected {}. Choose action: [A] Apply now  [E] Edit  [Back: Enter]", name);
                if let Some(act) = prompt_input() {
                    let t = act.trim().to_string();
                    if t.eq_ignore_ascii_case("a") {
                        // Check if it's a default or custom profile
                        let prof_opt = match name.as_str() {
                            "Day" => Some(HVACProfile::Day),
                            "Night" => Some(HVACProfile::Night),
                            "Sleep" => Some(HVACProfile::Sleep),
                            "Party" => Some(HVACProfile::Party),
                            "Vacation" => Some(HVACProfile::Vacation),
                            "Away" => Some(HVACProfile::Away),
                            _ => None, // Custom profile
                        };
                        
                        if let Some(prof) = prof_opt {
                            // Apply default profile
                            let mut hvac = HVACSystem::new(conn);
                            apply_profile(conn, &mut hvac, prof, admin_username, current_role);
                        } else {
                            // Apply custom profile
                            apply_custom_profile(conn, admin_username, current_role, selected_profile)?;
                        }
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

// Helper function to create a new profile (for homeowners and technicians)
fn create_new_profile_flow(conn: &mut Connection, username: &str, user_role: &str) -> Result<()> {
    use std::io::{self, Write};

    println!("\n========== CREATE NEW PROFILE ==========");

    // 1. Get profile name
    print!("Enter profile name (3-20 characters, letters/numbers/spaces only): ");
    io::stdout().flush()?;
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim().to_string();

    // Validate profile name
    if let Some(error) = db::validate_profile_name(conn, &name)? {
        println!("❌ Invalid profile name: {}", error);
        return Ok(());
    }

    // 2. Get mode
    println!("\nAvailable modes:");
    println!("  1) Off");
    println!("  2) Heating");
    println!("  3) Cooling");
    println!("  4) FanOnly");
    println!("  5) Auto");
    print!("Choose mode (1-5): ");
    io::stdout().flush()?;
    let mut mode_choice = String::new();
    io::stdin().read_line(&mut mode_choice)?;
    let mode = match mode_choice.trim() {
        "1" => "Off",
        "2" => "Heating",
        "3" => "Cooling",
        "4" => "FanOnly",
        "5" => "Auto",
        _ => {
            println!("❌ Invalid choice");
            return Ok(());
        }
    };

    // 3. Get target temperature
    print!("Enter target temperature (16-40°C): ");
    io::stdout().flush()?;
    let mut temp_str = String::new();
    io::stdin().read_line(&mut temp_str)?;
    let target_temp: f32 = match temp_str.trim().parse() {
        Ok(t) if t >= 16.0 && t <= 40.0 => t,
        _ => {
            println!("❌ Invalid temperature. Must be between 16-40°C");
            return Ok(());
        }
    };

    // 4. Get heater status
    println!("\nHeater status:");
    println!("  1) On");
    println!("  2) Off");
    println!("  3) Auto");
    print!("Choose heater status (1-3): ");
    io::stdout().flush()?;
    let mut heater_choice = String::new();
    io::stdin().read_line(&mut heater_choice)?;
    let heater_status = match heater_choice.trim() {
        "1" => "On",
        "2" => "Off",
        "3" => "Auto",
        _ => {
            println!("❌ Invalid choice");
            return Ok(());
        }
    };

    // 5. Get AC status
    println!("\nAC status:");
    println!("  1) On");
    println!("  2) Off");
    println!("  3) Auto");
    print!("Choose AC status (1-3): ");
    io::stdout().flush()?;
    let mut ac_choice = String::new();
    io::stdin().read_line(&mut ac_choice)?;
    let ac_status = match ac_choice.trim() {
        "1" => "On",
        "2" => "Off",
        "3" => "Auto",
        _ => {
            println!("❌ Invalid choice");
            return Ok(());
        }
    };

    // 6. Get light status
    println!("\nLight status:");
    println!("  1) ON");
    println!("  2) OFF");
    print!("Choose light status (1-2): ");
    io::stdout().flush()?;
    let mut light_choice = String::new();
    io::stdin().read_line(&mut light_choice)?;
    let light_status = match light_choice.trim() {
        "1" => "ON",
        "2" => "OFF",
        _ => {
            println!("❌ Invalid choice");
            return Ok(());
        }
    };

    // 7. Get fan speed
    println!("\nFan speed:");
    println!("  1) Low");
    println!("  2) Medium");
    println!("  3) High");
    print!("Choose fan speed (1-3): ");
    io::stdout().flush()?;
    let mut fan_choice = String::new();
    io::stdin().read_line(&mut fan_choice)?;
    let fan_speed = match fan_choice.trim() {
        "1" => "Low",
        "2" => "Medium",
        "3" => "High",
        _ => {
            println!("❌ Invalid choice");
            return Ok(());
        }
    };

    // 8. Optional: greeting and description
    print!("\nEnter greeting (optional, press Enter to skip): ");
    io::stdout().flush()?;
    let mut greeting = String::new();
    io::stdin().read_line(&mut greeting)?;
    let greeting = greeting.trim();

    print!("Enter description (optional, press Enter to skip): ");
    io::stdout().flush()?;
    let mut description = String::new();
    io::stdin().read_line(&mut description)?;
    let description = description.trim();

    // Create the profile
    db::create_profile(
        conn,
        &name,
        mode,
        target_temp,
        if greeting.is_empty() { None } else { Some(greeting) },
        if description.is_empty() { None } else { Some(description) },
        heater_status,
        ac_status,
        light_status,
        fan_speed,
    )?;

    // Log the creation
    let log_msg = format!("Profile '{}' created by {} ({})", name, username, user_role);
    logger::log_event(conn, username, None, "HVAC", Some(&log_msg))?;

    println!("\n✅ Profile '{}' created successfully!", name);
    Ok(())
}

// Helper function to delete a profile
fn delete_profile_flow(conn: &mut Connection, username: &str, user_role: &str) -> Result<()> {
    use std::io::{self, Write};

    println!("\n========== DELETE PROFILE ==========");

    // List all profiles with type indicator
    let profiles = db::list_profile_rows(conn)?;
    if profiles.is_empty() {
        println!("No profiles found.");
        return Ok(());
    }

    println!("\nAvailable Profiles:");
    for (idx, profile) in profiles.iter().enumerate() {
        let profile_type = if db::is_default_profile(&profile.name) {
            "[Default]"
        } else {
            "[Custom]"
        };
        println!("  {}. {} {}", idx + 1, profile.name, profile_type);
    }

    println!("\n⚠️  Note: Default profiles cannot be deleted.");

    // Get profile name to delete
    print!("\nEnter profile name to delete (or press Enter to cancel): ");
    io::stdout().flush()?;
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim();

    if name.is_empty() {
        println!("❌ Delete cancelled.");
        return Ok(());
    }

    // Check if it's a default profile
    if db::is_default_profile(name) {
        println!("❌ Cannot delete default profile '{}'", name);
        return Ok(());
    }

    // Check if profile exists
    if !profiles.iter().any(|p| p.name.eq_ignore_ascii_case(name)) {
        println!("❌ Profile '{}' not found", name);
        return Ok(());
    }

    // Confirm deletion
    print!("⚠️  Are you sure you want to delete profile '{}'? (yes/no): ", name);
    io::stdout().flush()?;
    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm)?;
    
    if confirm.trim().eq_ignore_ascii_case("yes") {
        db::delete_profile(conn, name)?;
        
        // Log the deletion
        let log_msg = format!("Profile '{}' deleted by {} ({})", name, username, user_role);
        logger::log_event(conn, username, None, "HVAC", Some(&log_msg))?;
        
        println!("✅ Profile '{}' deleted successfully!", name);
    } else {
        println!("❌ Delete cancelled.");
    }

    Ok(())
}

// Helper function to edit a profile with full control
fn edit_profile_full_flow(conn: &mut Connection, username: &str, user_role: &str) -> Result<()> {
    use std::io::{self, Write};

    println!("\n========== EDIT PROFILE ==========");

    // List all profiles
    let profiles = db::list_profile_rows(conn)?;
    if profiles.is_empty() {
        println!("No profiles found.");
        return Ok(());
    }

    println!("\nAvailable Profiles:");
    for (idx, profile) in profiles.iter().enumerate() {
        let profile_type = if db::is_default_profile(&profile.name) {
            "[Default]"
        } else {
            "[Custom]"
        };
        println!("  {}. {} {}", idx + 1, profile.name, profile_type);
    }

    // Get profile name to edit
    print!("\nEnter profile name to edit (or press Enter to cancel): ");
    io::stdout().flush()?;
    let mut name = String::new();
    io::stdin().read_line(&mut name)?;
    let name = name.trim();

    if name.is_empty() {
        println!("❌ Edit cancelled.");
        return Ok(());
    }

    // Load current profile
    let current = match db::get_profile_row(conn, name)? {
        Some(p) => p,
        None => {
            println!("❌ Profile '{}' not found", name);
            return Ok(());
        }
    };

    println!("\n--- Current Settings for '{}' ---", name);
    println!("Mode: {}", current.mode);
    println!("Target Temperature: {}°C", current.target_temp);
    println!("Heater: {}", current.heater_status);
    println!("AC: {}", current.ac_status);
    println!("Light: {}", current.light_status);
    println!("Fan Speed: {}", current.fan_speed);
    println!("Greeting: {}", current.greeting.as_deref().unwrap_or("(none)"));
    println!("Description: {}", current.description.as_deref().unwrap_or("(none)"));
    println!("\n📝 Press Enter to keep current value, or enter new value:\n");

    // 1. Edit mode
    print!("Mode [Off/Heating/Cooling/FanOnly/Auto] (current: {}): ", current.mode);
    io::stdout().flush()?;
    let mut mode_input = String::new();
    io::stdin().read_line(&mut mode_input)?;
    let new_mode = if mode_input.trim().is_empty() {
        current.mode
    } else {
        match mode_input.trim() {
            "Off" | "Heating" | "Cooling" | "FanOnly" | "Auto" => mode_input.trim().to_string(),
            _ => {
                println!("❌ Invalid mode. Keeping current value.");
                current.mode
            }
        }
    };

    // 2. Edit target temperature
    print!("Target Temperature [16-40] (current: {}): ", current.target_temp);
    io::stdout().flush()?;
    let mut temp_input = String::new();
    io::stdin().read_line(&mut temp_input)?;
    let new_target_temp = if temp_input.trim().is_empty() {
        current.target_temp
    } else {
        match temp_input.trim().parse::<f32>() {
            Ok(t) if t >= 16.0 && t <= 40.0 => t,
            _ => {
                println!("❌ Invalid temperature. Keeping current value.");
                current.target_temp
            }
        }
    };

    // 3. Edit heater status
    print!("Heater [On/Off/Auto] (current: {}): ", current.heater_status);
    io::stdout().flush()?;
    let mut heater_input = String::new();
    io::stdin().read_line(&mut heater_input)?;
    let new_heater = if heater_input.trim().is_empty() {
        current.heater_status
    } else {
        match heater_input.trim() {
            "On" | "Off" | "Auto" => heater_input.trim().to_string(),
            _ => {
                println!("❌ Invalid heater status. Keeping current value.");
                current.heater_status
            }
        }
    };

    // 4. Edit AC status
    print!("AC [On/Off/Auto] (current: {}): ", current.ac_status);
    io::stdout().flush()?;
    let mut ac_input = String::new();
    io::stdin().read_line(&mut ac_input)?;
    let new_ac = if ac_input.trim().is_empty() {
        current.ac_status
    } else {
        match ac_input.trim() {
            "On" | "Off" | "Auto" => ac_input.trim().to_string(),
            _ => {
                println!("❌ Invalid AC status. Keeping current value.");
                current.ac_status
            }
        }
    };

    // 5. Edit light status
    print!("Light [ON/OFF] (current: {}): ", current.light_status);
    io::stdout().flush()?;
    let mut light_input = String::new();
    io::stdin().read_line(&mut light_input)?;
    let new_light = if light_input.trim().is_empty() {
        current.light_status
    } else {
        match light_input.trim() {
            "ON" | "OFF" => light_input.trim().to_string(),
            _ => {
                println!("❌ Invalid light status. Keeping current value.");
                current.light_status
            }
        }
    };

    // 6. Edit fan speed
    print!("Fan Speed [Low/Medium/High] (current: {}): ", current.fan_speed);
    io::stdout().flush()?;
    let mut fan_input = String::new();
    io::stdin().read_line(&mut fan_input)?;
    let new_fan_speed = if fan_input.trim().is_empty() {
        current.fan_speed
    } else {
        match fan_input.trim() {
            "Low" | "Medium" | "High" => fan_input.trim().to_string(),
            _ => {
                println!("❌ Invalid fan speed. Keeping current value.");
                current.fan_speed
            }
        }
    };

    // 7. Edit greeting (optional)
    print!("Greeting (current: {}): ", current.greeting.as_deref().unwrap_or("(none)"));
    io::stdout().flush()?;
    let mut greeting_input = String::new();
    io::stdin().read_line(&mut greeting_input)?;
    let new_greeting = if greeting_input.trim().is_empty() {
        current.greeting
    } else {
        Some(greeting_input.trim().to_string())
    };

    // 8. Edit description (optional)
    print!("Description (current: {}): ", current.description.as_deref().unwrap_or("(none)"));
    io::stdout().flush()?;
    let mut desc_input = String::new();
    io::stdin().read_line(&mut desc_input)?;
    let new_description = if desc_input.trim().is_empty() {
        current.description
    } else {
        Some(desc_input.trim().to_string())
    };

    // Update the profile
    db::update_profile_full(
        conn,
        name,
        &new_mode,
        new_target_temp,
        new_greeting.as_deref(),
        new_description.as_deref(),
        &new_heater,
        &new_ac,
        &new_light,
        &new_fan_speed,
    )?;

    // Log the edit
    let log_msg = format!("Profile '{}' edited by {} ({})", name, username, user_role);
    logger::log_event(conn, username, None, "HVAC", Some(&log_msg))?;

    println!("\n✅ Profile '{}' updated successfully!", name);
    Ok(())
}

// Helper function to apply a custom profile
fn apply_custom_profile(
    conn: &mut Connection,
    username: &str,
    user_role: &str,
    profile: &db::ProfileRow,
) -> Result<()> {
    use crate::profile::celsius_to_fahrenheit;
    use chrono::Local;
    
    let mut hvac = HVACSystem::new(conn);
    
    // Map mode string to HVACMode
    let mode = match profile.mode.as_str() {
        "Off" => HVACMode::Off,
        "Heating" => HVACMode::Heating,
        "Cooling" => HVACMode::Cooling,
        "FanOnly" => HVACMode::FanOnly,
        "Auto" => HVACMode::Auto,
        _ => HVACMode::Auto,
    };
    
    let temperature = profile.target_temp;
    
    // Enforce mode-specific temperature ranges
    let (min_t, max_t) = mode.temperature_range();
    let adjusted_temp = if !mode.is_valid_temperature_for_mode(temperature) {
        let adjusted = if temperature < min_t { 
            min_t 
        } else if temperature > max_t { 
            max_t 
        } else { 
            temperature 
        };
        println!(
            "Note: Adjusted target temperature for {:?} mode to {:.1}°C (valid range {:.0}–{:.0}°C)",
            mode, adjusted, min_t, max_t
        );
        adjusted
    } else {
        temperature
    };
    
    // Apply the settings
    hvac.set_mode(conn, mode);
    hvac.set_target_temperature(conn, adjusted_temp);
    hvac.set_light_status(conn, &profile.light_status);
    
    // Display profile application
    let greet = profile.greeting.as_deref().unwrap_or("Custom profile activated");
    let now = Local::now();
    let time_str = now.format("%b %d, %Y %I:%M %p %Z").to_string();
    let temp_f = celsius_to_fahrenheit(adjusted_temp);
    
    // Get current temperature to determine actual runtime behavior
    let current_temp = senser::get_indoor_temperature().unwrap_or(22.0);
    
    // Determine heater/AC display status based on actual runtime behavior
    let (heater_display, ac_display) = match mode {
        HVACMode::Heating => ("ON", "OFF"),
        HVACMode::Cooling => ("OFF", "ON"),
        HVACMode::Auto => {
            // Auto mode: check temperature difference to determine what's actually running
            if current_temp < adjusted_temp - 0.5 {
                ("ON", "OFF")  // Need heating
            } else if current_temp > adjusted_temp + 0.5 {
                ("OFF", "ON")  // Need cooling
            } else {
                ("OFF", "OFF") // Temperature is at target
            }
        },
        HVACMode::FanOnly => ("OFF", "OFF"),
        HVACMode::Off => ("OFF", "OFF"),
    };
    
    println!("🌈✨=============================================✨🌈");
    println!("🏡  HVAC Profile Applied");
    println!();
    println!("📋  Profile: {} (Custom)", profile.name);
    println!();
    println!("{}", greet);
    println!();
    println!("⚙️  Mode: {:?}", mode);
    println!();
    println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", adjusted_temp, temp_f);
    println!();
    println!("📝  Description: Temperature: {:.1}°C / {:.1}°F", adjusted_temp, temp_f);
    println!("    🔥 Heater: {} | ❄️ AC: {} | 💡 Light: {} | 🌀 Fan: {}", 
             heater_display, ac_display, profile.light_status, profile.fan_speed);
    if let Some(desc) = &profile.description {
        println!("    {}", desc);
    }
    println!();
    println!("🕒  Time: {}", time_str);
    println!("🌈✨=============================================✨🌈");
    
    // Log the profile application
    let log_msg = format!(
        "Custom profile '{}' applied with mode {:?} and temp {:.1}°C",
        profile.name, mode, adjusted_temp
    );
    logger::log_event(conn, username, None, "HVAC", Some(&log_msg))?;
    
    let mode_str = format!("{:?}", mode);
    let _ = db::log_profile_applied(conn, username, user_role, &profile.name, &mode_str, adjusted_temp);
    
    Ok(())
}










