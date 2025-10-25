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

impl HVACSystem {
    pub fn new() -> Self {
        Self {
            mode: HVACMode::Off,
            target_temperature: 22.0,
        }
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
        self.target_temperature = temperature;
        let _ = logger::log_event(
            conn,
            "system",
            None,
            "HVAC",
            Some(&format!("Target temperature set to {:.1} °C", temperature)),
        );
    }

    pub fn update(&self, conn: &Connection) {
        let current_temp = match senser::get_indoor_temperature() {
            Ok(temp) => temp,
            Err(_) => {
                println!("Sensor error, defaulting to 22.0°C.");
                22.0
            }
        };

        match self.mode {
            HVACMode::Heating if current_temp < self.target_temperature => {
                println!("Heating ON → Current: {:.1}°C | Target: {:.1}°C", current_temp, self.target_temperature);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Heating activated"));
            }
            HVACMode::Cooling if current_temp > self.target_temperature => {
                println!("Cooling ON → Current: {:.1}°C | Target: {:.1}°C", current_temp, self.target_temperature);
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Cooling activated"));
            }
            HVACMode::FanOnly => {
                println!("Fan mode active.");
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Fan mode active"));
            }
            HVACMode::Auto => {
                if current_temp < self.target_temperature - 0.5 {
                    println!("Auto mode: Heating...");
                    let _ = logger::log_event(conn, "system", None, "HVAC", Some("Auto heating started"));
                } else if current_temp > self.target_temperature + 0.5 {
                    println!("Auto mode: Cooling...");
                    let _ = logger::log_event(conn, "system", None, "HVAC", Some("Auto cooling started"));
                } else {
                    println!("Auto mode: Stable at {:.1}°C", current_temp);
                }
            }
            _ => {
                println!("HVAC idle.");
                let _ = logger::log_event(conn, "system", None, "HVAC", Some("Idle state"));
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
