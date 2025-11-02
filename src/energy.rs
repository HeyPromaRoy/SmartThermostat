use anyhow::Result;
use chrono::{DateTime, Duration, Local, TimeZone};
use rand::Rng;
use rusqlite::{params, Connection};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EnergyUsage {
    pub timestamp: DateTime<Local>,
    pub energy_kwh: f64,
    pub mode: String, // "heating", "cooling", "fan", "off"
    pub temperature_delta: f32, // difference between target and actual
    pub duration_minutes: i32,
}

pub struct EnergyTracker;

impl EnergyTracker {
    /// Generate mock energy data for the past N days
    pub fn generate_mock_data(days: i64, _homeowner_username: &str) -> Vec<EnergyUsage> {
        let mut rng = rand::rng();
        let mut data = Vec::new();
        let now = Local::now();
        
        // Energy consumption patterns based on mode
        let mode_consumption: HashMap<&str, (f64, f64)> = [
            ("heating", (2.5, 4.0)),   // min, max kWh per hour
            ("cooling", (2.0, 3.5)),   // min, max kWh per hour
            ("fan", (0.1, 0.3)),       // min, max kWh per hour
            ("off", (0.0, 0.05)),      // minimal standby power
        ].iter().cloned().collect();

        for day in 0..days {
            let date = now - Duration::days(day);
            let date_naive = date.date_naive();
            
            // Generate 4-8 usage periods per day
            let periods = rng.random_range(4..=8);
            
            for _ in 0..periods {
                let hour = rng.random_range(0..24);
                let minute = rng.random_range(0..60);
                
                // Create NaiveDateTime and convert to DateTime<Local>
                let naive_timestamp = date_naive
                    .and_hms_opt(hour, minute, 0)
                    .unwrap_or_else(|| date_naive.and_hms_opt(12, 0, 0).unwrap());
                let timestamp = Local.from_local_datetime(&naive_timestamp).unwrap();
                
                let modes = vec!["heating", "cooling", "fan", "off"];
                let mode = modes[rng.random_range(0..modes.len())];
                
                let (min_kwh, max_kwh) = mode_consumption[mode];
                let duration = rng.random_range(15..180); // 15-180 minutes
                
                // Base energy + some randomness
                let hourly_rate = rng.random_range(min_kwh..=max_kwh);
                let energy_kwh = hourly_rate * (duration as f64 / 60.0);
                
                // Temperature delta affects energy usage
                let temp_delta = if mode == "heating" {
                    rng.random_range(-8.0..0.0) // heating when colder
                } else if mode == "cooling" {
                    rng.random_range(0.0..8.0) // cooling when warmer
                } else {
                    0.0
                };

                data.push(EnergyUsage {
                    timestamp,
                    energy_kwh,
                    mode: mode.to_string(),
                    temperature_delta: temp_delta,
                    duration_minutes: duration,
                });
            }
        }
        
        data.sort_by(|a, b| b.timestamp.cmp(&a.timestamp)); // newest first
        data
    }

    /// Calculate daily energy usage from raw data
    pub fn calculate_daily_usage(data: &[EnergyUsage]) -> HashMap<String, f64> {
        let mut daily_usage: HashMap<String, f64> = HashMap::new();
        
        for usage in data {
            let date_str = usage.timestamp.format("%Y-%m-%d").to_string();
            *daily_usage.entry(date_str).or_insert(0.0) += usage.energy_kwh;
        }
        
        daily_usage
    }

    /// Calculate mode-based energy usage
    pub fn calculate_mode_usage(data: &[EnergyUsage]) -> HashMap<String, f64> {
        let mut mode_usage: HashMap<String, f64> = HashMap::new();
        
        for usage in data {
            *mode_usage.entry(usage.mode.clone()).or_insert(0.0) += usage.energy_kwh;
        }
        
        mode_usage
    }

    /// Get energy efficiency rating (mock calculation)
    pub fn calculate_efficiency_rating(data: &[EnergyUsage]) -> String {
        if data.is_empty() {
            return "No Data".to_string();
        }
        
        let total_energy: f64 = data.iter().map(|d| d.energy_kwh).sum();
        let avg_daily_energy = total_energy / (data.len() as f64 / 4.0); // approximate days
        
        match avg_daily_energy {
            e if e < 5.0 => "Excellent ★★★★",
            e if e < 10.0 => "Good ★★★",
            e if e < 15.0 => "Average ★★",
            e if e < 20.0 => "Poor ★",
            _ => "Very Poor",
        }.to_string()
    }

    /// Display energy usage report
    pub fn display_energy_report(data: &[EnergyUsage]) {
        if data.is_empty() {
            println!("No energy usage data available.");
            return;
        }

        let daily_usage = Self::calculate_daily_usage(data);
        let mode_usage = Self::calculate_mode_usage(data);
        let efficiency = Self::calculate_efficiency_rating(data);
        
        let total_energy: f64 = data.iter().map(|d| d.energy_kwh).sum();
        let avg_daily: f64 = daily_usage.values().sum::<f64>() / daily_usage.len() as f64;
        
        println!("🌈✨=============================================✨🌈");
        println!("⚡  ENERGY USAGE REPORT");
        println!();
        println!("📊  Summary:");
        println!("   • Total Energy Used: {:.2} kWh", total_energy);
        println!("   • Average Daily: {:.2} kWh", avg_daily);
        println!("   • Efficiency Rating: {}", efficiency);
        println!("   • Period: {} days", daily_usage.len());
        println!();
        
        println!("🌡️  Usage by Mode:");
        let mut mode_vec: Vec<(&String, &f64)> = mode_usage.iter().collect();
        mode_vec.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
        
        for (mode, energy) in mode_vec {
            let percentage = (energy / total_energy * 100.0) as i32;
            println!("   • {:8}: {:5.1} kWh ({:2}%)", 
                match mode.as_str() {
                    "heating" => "Heating",
                    "cooling" => "Cooling", 
                    "fan" => "Fan",
                    "off" => "Standby",
                    _ => mode,
                },
                energy, 
                percentage
            );
        }
        println!();
        
        println!("📅  Daily Usage (Last 7 days):");
        let mut daily_vec: Vec<(&String, &f64)> = daily_usage.iter().collect();
        daily_vec.sort_by(|a, b| b.0.cmp(a.0)); // sort by date descending
        
        for (date, energy) in daily_vec.iter().take(7) {
            println!("   • {}: {:.1} kWh", date, energy);
        }
        
        println!("🌈✨=============================================✨🌈");
    }

    /// Store energy data in database (for historical tracking)
    pub fn store_energy_data(conn: &Connection, data: &[EnergyUsage], username: &str) -> Result<()> {
        // Create table if it doesn't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS energy_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                energy_kwh REAL NOT NULL,
                mode TEXT NOT NULL,
                temperature_delta REAL NOT NULL,
                duration_minutes INTEGER NOT NULL,
                recorded_at TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        // Insert energy data
        for usage in data {
            conn.execute(
                "INSERT INTO energy_usage (username, timestamp, energy_kwh, mode, temperature_delta, duration_minutes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    username,
                    usage.timestamp.to_rfc3339(),
                    usage.energy_kwh,
                    usage.mode,
                    usage.temperature_delta,
                    usage.duration_minutes
                ],
            )?;
        }

        Ok(())
    }

    /// Load historical energy data from database
    pub fn load_energy_data(conn: &Connection, username: &str, days: i64) -> Result<Vec<EnergyUsage>> {
        let cutoff = (Local::now() - Duration::days(days)).to_rfc3339();
        
        let mut stmt = conn.prepare(
            "SELECT timestamp, energy_kwh, mode, temperature_delta, duration_minutes 
             FROM energy_usage 
             WHERE username = ?1 AND timestamp > ?2 
             ORDER BY timestamp DESC"
        )?;
        
        let energy_iter = stmt.query_map(params![username, cutoff], |row| {
            let timestamp_str: String = row.get(0)?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .unwrap()
                .with_timezone(&Local);
            
            Ok(EnergyUsage {
                timestamp,
                energy_kwh: row.get(1)?,
                mode: row.get(2)?,
                temperature_delta: row.get(3)?,
                duration_minutes: row.get(4)?,
            })
        })?;

        let mut data = Vec::new();
        for energy in energy_iter {
            data.push(energy?);
        }

        Ok(data)
    }
}

/// Main function to view energy usage (called from menu)
pub fn view_energy_usage(conn: &Connection, username: &str) -> Result<()> {
    println!("\n📊 Generating energy usage report...");
    
    // Try to load existing data, or generate mock data
    let energy_data = match EnergyTracker::load_energy_data(conn, username, 30) {
        Ok(data) if !data.is_empty() => {
            println!("   Loaded historical data");
            data
        }
        _ => {
            println!("   Generating mock data for demonstration");
            let mock_data = EnergyTracker::generate_mock_data(30, username);
            // Store the mock data for future reference
            let _ = EnergyTracker::store_energy_data(conn, &mock_data, username);
            mock_data
        }
    };

    EnergyTracker::display_energy_report(&energy_data);
    
    Ok(())
}

/// Function to compare energy usage with previous period
pub fn compare_energy_usage(conn: &Connection, username: &str) -> Result<()> {
    let current_data = match EnergyTracker::load_energy_data(conn, username, 30) {
        Ok(data) if !data.is_empty() => data,
        _ => EnergyTracker::generate_mock_data(30, username),
    };

    let previous_data = match EnergyTracker::load_energy_data(conn, username, 60) {
        Ok(data) if data.len() > 30 => {
            data.into_iter()
                .filter(|d| d.timestamp < Local::now() - Duration::days(30))
                .collect()
        }
        _ => {
            let mut mock = EnergyTracker::generate_mock_data(60, username);
            mock.retain(|d| d.timestamp < Local::now() - Duration::days(30));
            mock
        }
    };

    let current_total: f64 = current_data.iter().map(|d| d.energy_kwh).sum();
    let previous_total: f64 = previous_data.iter().map(|d| d.energy_kwh).sum();
    
    let change = if previous_total > 0.0 {
        ((current_total - previous_total) / previous_total) * 100.0
    } else {
        0.0
    };

    println!("🌈✨=============================================✨🌈");
    println!("📈 ENERGY USAGE COMPARISON");
    println!();
    println!("Current Period (Last 30 days):");
    println!("   • Total Energy: {:.1} kWh", current_total);
    println!();
    println!("Previous Period (30-60 days ago):");
    println!("   • Total Energy: {:.1} kWh", previous_total);
    println!();
    println!("📊 Comparison:");
    println!("   • Change: {:.1}%", change);
    println!("   • Status: {}", 
        if change < -5.0 { "✅ Improving" }
        else if change > 5.0 { "⚠️  Increasing" }
        else { "➡️  Stable" }
    );
    println!("🌈✨=============================================✨🌈");

    Ok(())
}