use crate::hvac::{HVACMode, HVACSystem};
use rusqlite::Connection;
use crate::logger;

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
            HVACProfile::Vacation => (HVACMode::Off, 16.0),
            HVACProfile::Away => (HVACMode::Off, 17.0),
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
}

pub fn apply_profile(conn: &Connection, hvac: &mut HVACSystem, profile: HVACProfile) {
    let (mode, temperature) = profile.get_settings();
    hvac.set_mode(conn, mode);
    hvac.set_target_temperature(conn, temperature);
    hvac.update(conn);
    let _ = logger::log_event(
        conn,
        "system",
        None,
        "HVAC",
        Some(&format!("Profile '{}' applied with mode {:?} and temp {:.1}", format!("{:?}", profile), mode, temperature)),
    );
    println!("Applied profile: {}", profile.description());
}
