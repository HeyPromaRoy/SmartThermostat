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
            HVACProfile::Day => "Auto mode, comfort-oriented, 21-23°C / 24-26°C, Auto fan, Comfort",
            HVACProfile::Night => "Auto or steady heating/cooling, 20°C heating / 25°C cooling, Low fan speed, Moderate",
            HVACProfile::Sleep => "Heating preferred, quiet fan, 18-20°C heating / 26-28°C cooling, Fan off/low, Energy saving",
            HVACProfile::Party => "Cooling with ventilation, 22°C heating / 23-24°C cooling, Medium-high fan, Comfort prioritized",
            HVACProfile::Vacation => "HVAC mostly off, 16-18°C heating / 29-30°C cooling, Fan off, Max energy saving",
            HVACProfile::Away => "HVAC off/eco mode, 17-18°C heating / 28°C cooling, Fan off, Energy saving",
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
    
    // Display greeting and time/schedule first
    let greet = greeting_opt.as_deref().unwrap_or(profile.greeting_message());
    println!("\n{}", greet);
    let now = Local::now();
    println!("Current time: {}", now.format("%Y-%m-%d %H:%M:%S %Z"));
    let scheduled = current_scheduled_profile();
    if scheduled == profile {
        println!("Scheduled profile window: {:?} (within window ✅)", scheduled);
    } else {
        println!("Scheduled profile window: {:?} (you selected {:?})", scheduled, profile);
    }
    println!("");
    
    hvac.update(conn);
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
    
    let desc = description_opt.as_deref().unwrap_or(profile.description());
    println!("Applied profile: {}", desc);
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
