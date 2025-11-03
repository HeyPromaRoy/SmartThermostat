use rusqlite::Connection;
use chrono::Local;
use crate::logger;
use crate::senser;

/// Convert Celsius to Fahrenheit
fn celsius_to_fahrenheit(celsius: f32) -> f32 {
    (celsius * 9.0 / 5.0) + 32.0
}

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
    pub light_status: String,
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
    /// Create new HVAC system, loading state from database
    pub fn new(conn: &Connection) -> Self {
        // Try to load from database, fallback to default if error
        match crate::db::get_hvac_state(conn) {
            Ok((mode_str, temp, light)) => {
                let mode = match mode_str.as_str() {
                    "Heating" => HVACMode::Heating,
                    "Cooling" => HVACMode::Cooling,
                    "FanOnly" => HVACMode::FanOnly,
                    "Auto" => HVACMode::Auto,
                    _ => HVACMode::Off,
                };
                Self {
                    mode,
                    target_temperature: temp,
                    light_status: light,
                }
            }
            Err(_) => {
                // Fallback to default if database read fails
                Self {
                    mode: HVACMode::Off,
                    target_temperature: 22.0,
                    light_status: "OFF".to_string(),
                }
            }
        }
    }
    
    /// Validates if temperature is within allowed range
    #[allow(dead_code)]
    pub fn is_valid_temperature(temp: f32) -> bool {
        temp >= MIN_TEMPERATURE && temp <= MAX_TEMPERATURE
    }

    pub fn set_mode(&mut self, conn: &Connection, mode: HVACMode) {
        self.mode = mode;
        
        // Save to database
        let mode_str = match mode {
            HVACMode::Off => "Off",
            HVACMode::Heating => "Heating",
            HVACMode::Cooling => "Cooling",
            HVACMode::FanOnly => "FanOnly",
            HVACMode::Auto => "Auto",
        };
        let _ = crate::db::save_hvac_state(conn, mode_str, self.target_temperature, &self.light_status);
        
        let _ = logger::log_event(
            conn,
            "system",
            None,
            "HVAC",
            Some(&format!("Mode set to {:?}", mode)),
        );
    }
    
    pub fn set_light_status(&mut self, conn: &Connection, light_status: &str) {
        self.light_status = light_status.to_string();
        
        // Save to database
        let mode_str = match self.mode {
            HVACMode::Off => "Off",
            HVACMode::Heating => "Heating",
            HVACMode::Cooling => "Cooling",
            HVACMode::FanOnly => "FanOnly",
            HVACMode::Auto => "Auto",
        };
        let _ = crate::db::save_hvac_state(conn, mode_str, self.target_temperature, &self.light_status);
        
        let _ = logger::log_event(
            conn,
            "system",
            None,
            "HVAC",
            Some(&format!("Light status set to {}", light_status)),
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
        
        // Save to database
        let mode_str = match self.mode {
            HVACMode::Off => "Off",
            HVACMode::Heating => "Heating",
            HVACMode::Cooling => "Cooling",
            HVACMode::FanOnly => "FanOnly",
            HVACMode::Auto => "Auto",
        };
        let _ = crate::db::save_hvac_state(conn, mode_str, self.target_temperature, &self.light_status);
    }

    pub fn update(&self, conn: &Connection) {
        let current_temp = match senser::get_indoor_temperature() {
            Ok(temp) => temp,
            Err(_) => {
                println!("⚠️  Sensor error, defaulting to 22.0°C.");
                22.0
            }
        };

        let now = Local::now();
        let time_str = now.format("%b %d, %Y %I:%M %p %Z").to_string();

        let current_temp_f = celsius_to_fahrenheit(current_temp);
        let target_temp_f = celsius_to_fahrenheit(self.target_temperature);

        println!("🌈✨=============================================✨🌈");
        match self.mode {
            HVACMode::Heating if current_temp < self.target_temperature => {
                println!("🔥  HVAC Status: HEATING");
                println!();
                println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                println!();
                println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                println!();
                println!("⚙️  Mode: Heating");
                println!();
                println!("🔥  Heater: ON");
                println!();
                println!("❄️  AC: OFF");
                println!();
                println!("�  Light: {}", self.light_status);
                println!();
                println!("�📊  Status: Warming up your space!");
                println!();
                println!("🕒  Time: {}", time_str);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Heating activated"));
            }
            HVACMode::Heating => {
                println!("🔥  HVAC Status: HEATING");
                println!();
                println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                println!();
                println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                println!();
                println!("⚙️  Mode: Heating");
                println!();
                println!("🔥  Heater: ON");
                println!();
                println!("❄️  AC: OFF");
                println!();
                println!("�  Light: {}", self.light_status);
                println!();
                println!("�📊  Status: Temperature reached!");
                println!();
                println!("🕒  Time: {}", time_str);
            }
            HVACMode::Cooling if current_temp > self.target_temperature => {
                println!("❄️  HVAC Status: COOLING");
                println!();
                println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                println!();
                println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                println!();
                println!("⚙️  Mode: Cooling");
                println!();
                println!("🔥  Heater: OFF");
                println!();
                println!("❄️  AC: ON");
                println!();
                println!("�  Light: {}", self.light_status);
                println!();
                println!("�📊  Status: AC cooling down your space!");
                println!();
                println!("🕒  Time: {}", time_str);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Cooling activated"));
            }
            HVACMode::Cooling => {
                println!("❄️  HVAC Status: COOLING");
                println!();
                println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                println!();
                println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                println!();
                println!("⚙️  Mode: Cooling");
                println!();
                println!("🔥  Heater: OFF");
                println!();
                println!("❄️  AC: ON");
                println!();
                println!("💡  Light: {}", self.light_status);
                println!();
                println!("📊  Status: Temperature reached!");
                println!();
                println!("🕒  Time: {}", time_str);
            }
            HVACMode::FanOnly => {
                println!("💨  HVAC Status: FAN ONLY");
                println!();
                println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                println!();
                println!("⚙️  Mode: Fan Only");
                println!();
                println!("🔥  Heater: OFF");
                println!();
                println!("❄️  AC: OFF");
                println!();
                println!("�  Light: {}", self.light_status);
                println!();
                println!("�💨  Fan: ON");
                println!();
                println!("📊  Status: Circulating fresh air!");
                println!();
                println!("🕒  Time: {}", time_str);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Fan mode active"));
            }
            HVACMode::Auto => {
                if current_temp < self.target_temperature - 0.5 {
                    println!("🤖  HVAC Status: AUTO MODE");
                    println!();
                    println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                    println!();
                    println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                    println!();
                    println!("⚙️  Mode: Auto");
                    println!();
                    println!("🔥  Heater: ON");
                    println!();
                    println!("❄️  AC: OFF");
                    println!();
                    println!("�  Light: {}", self.light_status);
                    println!();
                    println!("�📊  Status: Heating to reach target");
                    println!();
                    println!("🕒  Time: {}", time_str);
                    let _ = logger::log_event(conn, "system", None, "HVAC", Some("Auto heating started"));
                } else if current_temp > self.target_temperature + 0.5 {
                    println!("🤖  HVAC Status: AUTO MODE");
                    println!();
                    println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                    println!();
                    println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                    println!();
                    println!("⚙️  Mode: Auto");
                    println!();
                    println!("🔥  Heater: OFF");
                    println!();
                    println!("❄️  AC: ON");
                    println!();
                    println!("💡  Light: {}", self.light_status);
                    println!();
                    println!("📊  Status: Cooling to reach target");
                    println!();
                    println!("🕒  Time: {}", time_str);
                    let _ = logger::log_event(conn, "system", None, "HVAC", Some("Auto cooling started"));
                } else {
                    println!("🤖  HVAC Status: AUTO MODE");
                    println!();
                    println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                    println!();
                    println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
                    println!();
                    println!("⚙️  Mode: Auto");
                    println!();
                    println!("🔥  Heater: OFF");
                    println!();
                    println!("❄️  AC: OFF");
                    println!();
                    println!("💡  Light: {}", self.light_status);
                    println!();
                    println!("📊  Status: Maintaining comfort (Perfect temp!)");
                    println!();
                    println!("🕒  Time: {}", time_str);
                }
            }
            HVACMode::Off => {
                println!("⭕  HVAC Status: OFF");
                println!();
                println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
                println!();
                println!("⚙️  Mode: Off");
                println!();
                println!("🔥  Heater: OFF");
                println!();
                println!("❄️  AC: OFF");
                println!();
                println!("�  Light: {}", self.light_status);
                println!();
                println!("�💨  Fan: OFF");
                println!();
                println!("📊  Status: No climate control");
                println!();
                println!("🕒  Time: {}", time_str);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("System off"));
            }
        }
        println!("🌈✨=============================================✨🌈");
    }

    pub fn diagnostics(&self, conn: &Connection) {
        let current_temp = match senser::get_indoor_temperature() {
            Ok(temp) => temp,
            Err(_) => 22.0,
        };
        
        let current_temp_f = celsius_to_fahrenheit(current_temp);
        let target_temp_f = celsius_to_fahrenheit(self.target_temperature);
        let now = Local::now();
        let time_str = now.format("%b %d, %Y %I:%M %p %Z").to_string();
        
        println!("🌈✨=============================================✨🌈");
        println!("🔧  HVAC System Diagnostics");
        println!();
        println!("⚙️  Mode: {:?}", self.mode);
        println!();
        println!("🎯  Target Temperature: {:.1}°C / {:.1}°F", self.target_temperature, target_temp_f);
        println!();
        println!("🌡️  Current Temperature: {:.1}°C / {:.1}°F", current_temp, current_temp_f);
        println!();
        
        let (min_temp, max_temp) = self.mode.temperature_range();
        let min_temp_f = celsius_to_fahrenheit(min_temp);
        let max_temp_f = celsius_to_fahrenheit(max_temp);
        println!("📏  Valid Range: {:.0}°C - {:.0}°C / {:.0}°F - {:.0}°F", min_temp, max_temp, min_temp_f, max_temp_f);
        println!();
        println!("🕒  Time: {}", time_str);
        println!("🌈✨=============================================✨🌈");
        
        let _ = logger::log_event(conn, "system", None, "HVAC", Some("Diagnostics executed"));
    }
}
