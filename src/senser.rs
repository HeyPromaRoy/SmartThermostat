// src/senser.rs
//! Purpose：Get all the indoor senser data(temperature、humidity、CO level)
//! - input validation(boundary、NaN、infinite) and error handling(prevent panic)

use rand::Rng;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Senser type
#[derive(Debug, Clone, Copy)]
pub enum SensorType {
    TemperatureC, // C(degree)
    HumidityPct,  // %
    COPpm,        // CO ppm
}

/// data unit
#[derive(Debug, Clone, Copy)]
pub struct IndoorReading {
    pub temperature_c: f32,
    pub humidity_pct: f32,
    pub co_ppm: f32,
}

/// Error type
#[derive(Debug)]
pub enum SensorError {
    InvalidBounds {
        lower: f32,
        upper: f32,
        reason: &'static str,
    },
    InvalidInput(&'static str),
    DataSource(&'static str),
}

/// Dashboard for indoor data
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub temp_warn_hi: f32, // °C
    pub co_warn_hi: f32,   // ppm
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            temp_warn_hi: 32.0, // Warning for high temperature
            co_warn_hi: 35.0,   // Warning for high CO level
        }
    }
}

impl fmt::Display for SensorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SensorError::InvalidBounds { lower, upper, reason } => {
                write!(f, "Invalid bounds: [{lower}, {upper}] - {reason}")
            }
            SensorError::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
            SensorError::DataSource(msg) => write!(f, "Data source error: {msg}"),
        }
    }
}

impl std::error::Error for SensorError {}

/// make sure f32 is not NaN/infinite
fn validate_finite(x: f32) -> Result<(), SensorError> {
    if !x.is_finite() {
        return Err(SensorError::InvalidInput("non-finite float"));
    }
    Ok(())
}

/// make sure that data range is resonable
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo { lo } else if v > hi { hi } else { v }
}

/// set default boundary for each senser(upper/lower bound)
fn default_bounds(kind: SensorType) -> (f32, f32) {
    match kind {
        SensorType::TemperatureC => (-40.0, 85.0),
        SensorType::HumidityPct  => (0.0, 100.0),
        SensorType::COPpm        => (0.0, 1000.0),
    }
}

/// Generate random data to simulate the source data from senser
/// input: type of senser, lower bound, upper bound
/// output: number(f32)
/// - check lower < upper, and infinite
/// - output range is between [lower, upper]
pub fn gen_random_data(
    sensor: SensorType,
    lower: f32,
    upper: f32,
) -> Result<f32, SensorError> {
    validate_finite(lower)?;
    validate_finite(upper)?;
    if !(lower < upper) {
        return Err(SensorError::InvalidBounds {
            lower, upper, reason: "lower must be strictly less than upper",
        });
    }

    let (dlo, dup) = default_bounds(sensor);
    let lo = lower.max(dlo);
    let hi = upper.min(dup);

    if !(lo < hi) {
        return Err(SensorError::InvalidBounds {
            lower, upper, reason: "provided range has no overlap with default safe range",
        });
    }

    let mut rng = rand::rng();
    let v: f32 = rng.gen_range(lo..hi);
    Ok(v)
}

/// Get the indoor temperature(°C)
/// output: number(f32)
pub fn get_indoor_temperature() -> Result<f32, SensorError> {
    let (lo, hi) = default_bounds(SensorType::TemperatureC);
    let samples = 3;
    let mut acc = 0.0f32;
    for _ in 0..samples {
        acc += gen_random_data(SensorType::TemperatureC, lo, hi)?;
    }
    let avg = acc / samples as f32;
    Ok(clamp(avg, lo, hi))
}

/// Get the indoor humidiry(%)
/// output: numberf32)
pub fn get_indoor_humidity() -> Result<f32, SensorError> {
    let (lo, hi) = default_bounds(SensorType::HumidityPct);
    let v = gen_random_data(SensorType::HumidityPct, lo, hi)?;
    Ok(clamp(v, lo, hi))
}

/// Get the indoor CO level(ppm)
/// output: number(f32)
pub fn get_indoor_colevel() -> Result<f32, SensorError> {
    let (lo, hi) = default_bounds(SensorType::COPpm);
    let v = gen_random_data(SensorType::COPpm, lo, hi)?;
    Ok(clamp(v, lo, hi))
}

/// Get all the indoor data with time
pub fn read_all() -> Result<IndoorReading, SensorError> {
    let temperature_c = get_indoor_temperature()?;
    let humidity_pct = get_indoor_humidity()?;
    let co_ppm = get_indoor_colevel()?;
    let _ts_sec = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SensorError::DataSource("system clock error"))?
        .as_secs();

    Ok(IndoorReading {
        temperature_c,
        humidity_pct,
        co_ppm,
    })
}

pub fn run_dashboard_inline(thresholds: Thresholds) -> Result<(), SensorError> {
    match read_all() {
        Ok(r) => {
            let red = |s: String| format!("\x1b[31m{}\x1b[0m", s);
            let green = |s: String| format!("\x1b[32m{}\x1b[0m", s);

            let temp_str = if r.temperature_c >= thresholds.temp_warn_hi {
                red(format!("{:.1}°C", r.temperature_c))
            } else {
                green(format!("{:.1}°C", r.temperature_c))
            };

            let co_str = if r.co_ppm >= thresholds.co_warn_hi {
                red(format!("{:.1}ppm", r.co_ppm))
            } else {
                green(format!("{:.1}ppm", r.co_ppm))
            };

            let ts = chrono::Local::now();
            let formatted = ts.format("%b %d, %Y %I:%M %p %Z").to_string();

            println!("🌈✨=============================================✨🌈");
            println!("🏠  Indoor Air Status");
            println!("🌡️  Temperature: {}", temp_str);
            println!("💦  Humidity: {:.1}%", r.humidity_pct);
            println!("🫧  CO: {}", co_str);
            println!("🕒  Time: {}", formatted);
            println!("🌈✨=============================================✨🌈");
        }
        Err(e) => {
            println!("🌈✨=============================================✨🌈");
            println!("⚠️  Read error: {}", e);
            let ts = chrono::Local::now();
            let formatted = ts.format("%b %d, %Y %I:%M %p %Z").to_string();
            println!("🕒  Time: {}", formatted);
            println!("🌈✨=============================================✨🌈");
        }
    }

    Ok(())
}
