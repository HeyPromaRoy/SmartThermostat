use crate::hvac::{HVACMode, HVACSystem};
use rusqlite::Connection;
use crate::logger;
use crate::db;
use chrono::{Local, Timelike};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HVACProfile {
    Day,
    Night,
    Sleep,
    Party,
    Vacation,
    Away,
}

impl HVACProfile {
    pub fn get_settings(self) -> (HVACMode, f32) {
        match self {
            HVACProfile::Day => (HVACMode::Auto, 22.0),
            HVACProfile::Night => (HVACMode::Auto, 20.0),
            HVACProfile::Sleep => (HVACMode::Heating, 18.0),
            HVACProfile::Party => (HVACMode::Cooling, 23.0),
            HVACProfile::Vacation => (HVACMode::Off, 24.0),
            HVACProfile::Away => (HVACMode::Off, 25.0),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            HVACProfile::Day => "Auto mode, comfort-oriented\n     21-23°C heating / 24-26°C cooling\n     Auto fan speed\n     Comfort prioritized\n     Heater: Auto | AC: Auto",
            HVACProfile::Night => "Auto or steady heating/cooling\n     20°C heating / 25°C cooling\n     Low fan speed\n     Moderate comfort\n     Heater: Auto | AC: Auto",
            HVACProfile::Sleep => "Heating preferred, quiet operation\n     18-20°C heating / 26-28°C cooling\n     Fan off/low speed\n     Energy saving mode\n     Heater: On | AC: Off",
            HVACProfile::Party => "Cooling with ventilation\n     22°C heating / 23-24°C cooling\n     Medium-high fan speed\n     Comfort prioritized\n     Heater: Off | AC: On",
            HVACProfile::Vacation => "HVAC mostly off\n     16-18°C heating / 29-30°C cooling\n     Fan off\n     Max energy saving\n     Heater: Off | AC: Off",
            HVACProfile::Away => "HVAC off/eco mode\n     17-18°C heating / 28°C cooling\n     Fan off\n     Energy saving\n     Heater: Off | AC: Off",
        }
    }
    
    pub fn greeting_message(self) -> &'static str {
        match self {
            HVACProfile::Day => "☀️ Hope you have a good day!",
            HVACProfile::Night => "🌙 Have a Good Night!",
            HVACProfile::Sleep => "😴 Sleep well and sweet dreams!",
            HVACProfile::Party => "🎊 Let's get this party started!",
            HVACProfile::Vacation => "🏖️ Enjoy your vacation!",
            HVACProfile::Away => "🚗 Have a safe trip!",
        }
    }
}

pub fn apply_profile(conn: &Connection, hvac: &mut HVACSystem, profile: HVACProfile, username: &str, user_role: &str) {
    // Try DB override first
    let (mut mode, mut temperature) = profile.get_settings();

    let name = format!("{:?}", profile);
    let mut greeting_opt: Option<String> = None;
    let mut description_opt: Option<String> = None;
    if let Ok(Some(row)) = db::get_profile_row(conn, &name) {
        // Map mode string -> HVACMode
        mode = match row.mode.as_str() {
            "Off" => HVACMode::Off,
            "Heating" => HVACMode::Heating,
            "Cooling" => HVACMode::Cooling,
            "FanOnly" => HVACMode::FanOnly,
            "Auto" => HVACMode::Auto,
            _ => mode,
        };
        temperature = row.target_temp;
        greeting_opt = row.greeting;
        description_opt = row.description;
    }
    
    // Enforce mode-specific temperature ranges (e.g., Heating 25–32, Cooling 16–22)
    let (min_t, max_t) = mode.temperature_range();
    if !mode.is_valid_temperature_for_mode(temperature) {
        let adjusted = if temperature < min_t { min_t } else if temperature > max_t { max_t } else { temperature };
        println!(
            "Note: Adjusted target temperature for {:?} mode to {:.1}°C (valid range {:.0}–{:.0}°C)",
            mode, adjusted, min_t, max_t
        );
        temperature = adjusted;
    }

    hvac.set_mode(conn, mode);
    hvac.set_target_temperature(conn, temperature);
    
    // Display profile application with decorative format
    let greet = greeting_opt.as_deref().unwrap_or(profile.greeting_message());
    let now = Local::now();
    let time_str = now.format("%b %d, %Y %I:%M %p %Z").to_string();
    let scheduled = current_scheduled_profile();
    let desc = description_opt.as_deref().unwrap_or(profile.description());
    
    println!("🌈✨=============================================✨🌈");
    println!("🏡  HVAC Profile Applied");
    println!();
    println!("📋  Profile: {:?}", profile);
    println!();
    println!("{}", greet);
    println!();
    println!("⚙️  Mode: {:?}", mode);
    println!();
    println!("🎯  Target Temperature: {:.1}°C", temperature);
    println!();
    
    if scheduled == profile {
        println!("⏰  Schedule: Within {:?} window ✅", scheduled);
    } else {
        println!("⏰  Schedule: {:?} window (manual override)", scheduled);
    }
    println!();
    println!("📝  Description: {}", desc);
    println!();
    println!("🕒  Time: {}", time_str);
    println!("🌈✨=============================================✨🌈");
    
    let profile_name = name.clone();
    
    // Log to security_log (existing)
    let _ = logger::log_event(
        conn,
        "system",
        None,
        "HVAC",
        Some(&format!("Profile '{}' applied with mode {:?} and temp {:.1}", profile_name, mode, temperature)),
    );
    
    // Log to HVAC activity log (new tracking)
    let mode_str = format!("{:?}", mode);
    let _ = db::log_profile_applied(conn, username, user_role, &profile_name, &mode_str, temperature);
}

// Determine current scheduled profile based on local time windows.
// Assumptions (to avoid gaps):
// - Day: 06:00–18:00
// - Night: 18:01–23:00
// - Sleep: 23:01–05:59
pub fn current_scheduled_profile() -> HVACProfile {
    let now = Local::now();
    let hour = now.hour();
    let minute = now.minute();

    // Day window 06:00–18:00 inclusive
    if (6..=18).contains(&hour) {
        // If exactly 18:01 and beyond, handled below
        if hour == 18 && minute > 0 {
            // fall through
        } else {
            return HVACProfile::Day;
        }
    }

    // Night window 18:01–23:00
    if (18..=23).contains(&hour) {
        if (hour > 18) || (hour == 18 && minute >= 1) {
            if hour == 23 && minute > 0 {
                // 23:01 enters Sleep
            } else {
                return HVACProfile::Night;
            }
        }
    }

    // Sleep 23:01–05:59
    HVACProfile::Sleep
}
