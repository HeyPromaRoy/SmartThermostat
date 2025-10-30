use rusqlite::Connection;
use chrono::Local;
use crate::logger;
use crate::senser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HVACMode {
    Off,
    Heating,
    Cooling,
    FanOnly,
    Auto,
}

pub struct HVACSystem {
    pub mode: HVACMode,
    pub target_temperature: f32,
}

// Temperature limits constants
pub const MIN_TEMPERATURE: f32 = 16.0;
pub const MAX_TEMPERATURE: f32 = 40.0;

// Temperature ranges for each mode
// Updated per request: Heating 25–32°C, Cooling 16–22°C
pub const HEATING_MIN: f32 = 25.0;
pub const HEATING_MAX: f32 = 32.0;
pub const COOLING_MIN: f32 = 16.0;
pub const COOLING_MAX: f32 = 22.0;
pub const AUTO_MIN: f32 = 18.0;
pub const AUTO_MAX: f32 = 28.0;

impl HVACMode {
    /// Get the temperature range for a specific mode
    pub fn temperature_range(&self) -> (f32, f32) {
        match self {
            HVACMode::Heating => (HEATING_MIN, HEATING_MAX),
            HVACMode::Cooling => (COOLING_MIN, COOLING_MAX),
            HVACMode::Auto => (AUTO_MIN, AUTO_MAX),
            HVACMode::FanOnly | HVACMode::Off => (MIN_TEMPERATURE, MAX_TEMPERATURE),
        }
    }

    /// Check if temperature is valid for this mode
    pub fn is_valid_temperature_for_mode(&self, temp: f32) -> bool {
        let (min, max) = self.temperature_range();
        temp >= min && temp <= max
    }
}

impl HVACSystem {
    pub fn new() -> Self {
        Self {
            mode: HVACMode::Off,
            target_temperature: 22.0,
        }
    }
    
    /// Validates if temperature is within allowed range
    pub fn is_valid_temperature(temp: f32) -> bool {
        temp >= MIN_TEMPERATURE && temp <= MAX_TEMPERATURE
    }

    pub fn set_mode(&mut self, conn: &Connection, mode: HVACMode) {
        self.mode = mode;
        let _ = logger::log_event(
            conn,
            "system",
            None,
            "HVAC",
            Some(&format!("Mode set to {:?}", mode)),
        );
    }

    pub fn set_target_temperature(&mut self, conn: &Connection, temperature: f32) {
        // Validate temperature limits
        if temperature < MIN_TEMPERATURE {
            println!("❌ Temperature too low! Minimum allowed: {:.1}°C", MIN_TEMPERATURE);
            println!("   Setting to minimum: {:.1}°C", MIN_TEMPERATURE);
            self.target_temperature = MIN_TEMPERATURE;
            let _ = logger::log_event(
                conn,
                "system",
                None,
                "HVAC",
                Some(&format!("Temperature below limit ({:.1}°C), set to minimum {:.1}°C", temperature, MIN_TEMPERATURE)),
            );
        } else if temperature > MAX_TEMPERATURE {
            println!("❌ Temperature too high! Maximum allowed: {:.1}°C", MAX_TEMPERATURE);
            println!("   Setting to maximum: {:.1}°C", MAX_TEMPERATURE);
            self.target_temperature = MAX_TEMPERATURE;
            let _ = logger::log_event(
                conn,
                "system",
                None,
                "HVAC",
                Some(&format!("Temperature above limit ({:.1}°C), set to maximum {:.1}°C", temperature, MAX_TEMPERATURE)),
            );
        } else {
            self.target_temperature = temperature;
            let _ = logger::log_event(
                conn,
                "system",
                None,
                "HVAC",
                Some(&format!("Target temperature set to {:.1}°C", temperature)),
            );
        }
    }

    pub fn update(&self, conn: &Connection) {
        let current_temp = match senser::get_indoor_temperature() {
            Ok(temp) => temp,
            Err(_) => {
                println!("⚠️  Sensor error, defaulting to 22.0°C.");
                22.0
            }
        };

        match self.mode {
            HVACMode::Heating if current_temp < self.target_temperature => {
                println!("\n🔥 HEATER ON → Warming up your space!");
                println!("   Current: {:.1}°C | Target: {:.1}°C | Mode: Heating", current_temp, self.target_temperature);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Heating activated"));
            }
            HVACMode::Heating => {
                println!("\n🔥 Heater running → Temperature reached!");
                println!("   Current: {:.1}°C | Target: {:.1}°C | Mode: Heating", current_temp, self.target_temperature);
            }
            HVACMode::Cooling if current_temp > self.target_temperature => {
                println!("\n❄️  AC ON → Cooling down your space!");
                println!("   Current: {:.1}°C | Target: {:.1}°C | Mode: Cooling", current_temp, self.target_temperature);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Cooling activated"));
            }
            HVACMode::Cooling => {
                println!("\n❄️  AC running → Temperature reached!");
                println!("   Current: {:.1}°C | Target: {:.1}°C | Mode: Cooling", current_temp, self.target_temperature);
            }
            HVACMode::FanOnly => {
                println!("\n💨 FAN ONLY MODE → Circulating fresh air!");
                println!("   🔥 Heater: OFF  |  ❄️  AC: OFF  |  💨 Fan: ON");
                println!("   Current: {:.1}°C (no temperature control)", current_temp);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Fan mode active"));
            }
            HVACMode::Auto => {
                if current_temp < self.target_temperature - 0.5 {
                    println!("\n🤖 AUTO MODE → 🔥 Heater activated!");
                    println!("   Current: {:.1}°C | Target: {:.1}°C | Heating to reach target", current_temp, self.target_temperature);
                    let _ = logger::log_event(conn, "system", None, "HVAC", Some("Auto heating started"));
                } else if current_temp > self.target_temperature + 0.5 {
                    println!("\n🤖 AUTO MODE → ❄️  AC activated!");
                    println!("   Current: {:.1}°C | Target: {:.1}°C | Cooling to reach target", current_temp, self.target_temperature);
                    let _ = logger::log_event(conn, "system", None, "HVAC", Some("Auto cooling started"));
                } else {
                    println!("\n🤖 AUTO MODE → ✅ Perfect temperature!");
                    println!("   Current: {:.1}°C | Target: {:.1}°C | Maintaining comfort", current_temp, self.target_temperature);
                }
            }
            HVACMode::Off => {
                println!("\n⭕ HVAC SYSTEM OFF");
                println!("   🔥 Heater: OFF  |  ❄️  AC: OFF  |  💨 Fan: OFF");
                println!("   Current: {:.1}°C (no climate control)", current_temp);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("System off"));
            }
        }
    }

    pub fn diagnostics(&self, conn: &Connection) {
        println!("\n==== HVAC Diagnostics ====");
        println!("Mode: {:?}", self.mode);
        println!("Target Temperature: {:.1}°C", self.target_temperature);
        println!("Timestamp: {}", Local::now().format("%H:%M:%S"));
        let _ = logger::log_event(conn, "system", None, "HVAC", Some("Diagnostics executed"));
    }
}
