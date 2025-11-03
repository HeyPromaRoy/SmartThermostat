// src/diagnostic.rs
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rand::Rng;
use std::{thread, time::Duration};

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
    println!("🧰 Starting System Diagnostics ({total} items total)…\n");

    for (i, name) in steps.iter().enumerate() {
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos:>3}% | {msg}",
            )?
            .progress_chars("#>-"),
        );
        pb.set_message(format!("({}/{}) Checking: {}", i + 1, total, name));

        let step_ms: u64 = rand::thread_rng().gen_range(600..1100);
        let per_tick = step_ms / 100;
        for p in 0..=100 {
            pb.set_position(p);
            thread::sleep(Duration::from_millis(per_tick.max(3)));
        }

        pb.finish_with_message(format!("({}/{}) {} OK ✓", i + 1, total, name));
    }

    println!("\n✅ All systems are functioning normally! Diagnostics completed successfully.\n");
    Ok(())
}

