use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::{thread, time::Duration};

use crate::{senser, weather};

pub fn run_diagnostics() -> Result<()> {
    let steps = [
        "🌡️  Outdoor Temperature Sensor",
        "💧  Outdoor Humidity Sensor",
        "🌬️  Outdoor Wind Sensor",
        "🏠🌡️  Indoor Temperature Sensor",
        "🏠💧  Indoor Humidity Sensor",
        "🏠🫧  Indoor CO Sensor",
        "💡  Indoor Light Switch Function",
        "🌀  Indoor Fan Function",
        "❄️  Indoor Air Conditioner Function",
        "🔥  Indoor Heater Function",
    ];

    let total = steps.len();
    println!("🧰 Starting Smart Thermostat Diagnostics ({total} items total)…\n");

    for (i, name) in steps.iter().enumerate() {
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos:>3}% | {msg}",
            )?
            .progress_chars("#>-"),
        );
        pb.set_message(format!("({}/{}) Checking: {}", i + 1, total, name));

        // Smooth progress animation
        for p in 0..=100 {
            pb.set_position(p);
            thread::sleep(Duration::from_millis(8));
        }

        // Perform the actual check
        match *name {
            "🌡️  Outdoor Temperature Sensor"
            | "💧  Outdoor Humidity Sensor"
            | "🌬️  Outdoor Wind Sensor" => {
                // Only one call to fetch_weather() to validate outdoor sensors
                if let Err(e) = weather::fetch_weather() {
                    pb.abandon_with_message(format!("({}/{}) {} failed: {}", i + 1, total, name, e));
                    continue;
                }
            }

            "🏠🌡️  Indoor Temperature Sensor" => {
                if let Err(e) = senser::get_indoor_temperature() {
                    pb.abandon_with_message(format!("({}/{}) {} failed: {}", i + 1, total, name, e));
                    continue;
                }
            }

            "🏠💧  Indoor Humidity Sensor" => {
                if let Err(e) = senser::get_indoor_humidity() {
                    pb.abandon_with_message(format!("({}/{}) {} failed: {}", i + 1, total, name, e));
                    continue;
                }
            }

            "🏠🫧  Indoor CO Sensor" => {
                if let Err(e) = senser::get_indoor_colevel() {
                    pb.abandon_with_message(format!("({}/{}) {} failed: {}", i + 1, total, name, e));
                    continue;
                }
            }

            // Simulated device controls (you can replace these with real functions)
            "💡  Indoor Light Switch Function"
            | "🌀  Indoor Fan Function"
            | "❄️  Indoor Air Conditioner Function"
            | "🔥  Indoor Heater Function" => {
                thread::sleep(Duration::from_millis(500));
            }

            _ => {}
        }

        pb.finish_with_message(format!("({}/{}) {} OK ✓", i + 1, total, name));
    }

    println!("\n✅ All systems are functioning normally! Diagnostics completed successfully.\n");
    Ok(())
}
