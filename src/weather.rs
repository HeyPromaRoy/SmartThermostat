use anyhow::{Result, Context};
use rusqlite::{params, Connection};
use chrono::{DateTime, Utc};
use chrono_tz::America::New_York;
use serde::Deserialize;

// -----------------------------------------------
// NOAA observation API response structures
// -----------------------------------------------
#[derive(Deserialize, Debug)]
struct ObservationResponse {
    properties: ObservationProperties,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ObservationProperties {
    temperature: Option<Measurement>,
    dewpoint: Option<Measurement>,
    relative_humidity: Option<Measurement>,
    wind_speed: Option<Measurement>,
    wind_direction: Option<Measurement>,
    text_description: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    value: Option<f64>,
}

// -----------------------------------------------
// Internal data structure for storage
// -----------------------------------------------
#[derive(Debug)]
pub struct WeatherRecord {
    pub time: String,
    pub temperature_f: Option<f64>,
    pub temperature_c: Option<f64>,
    pub dewpoint_f: Option<f64>,
    pub dewpoint_c: Option<f64>,
    pub humidity: Option<f64>,
    pub wind_speed_mph: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub condition: String,
}

// -----------------------------------------------
// Database initialization
// -----------------------------------------------
fn open_or_init_db() -> Result<Connection> {
    let conn = Connection::open("weather.db").context("opening DB")?;
    conn.pragma_update(None, "journal_mode", &"WAL").ok();
    conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS weather (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            time TEXT,
            temperature_f REAL,
            temperature_c REAL,
            dewpoint_f REAL,
            dewpoint_c REAL,
            humidity REAL,
            wind_speed_mph REAL,
            wind_direction_deg REAL,
            condition TEXT
        )",
        [],
    )?;
    Ok(conn)
}

// -----------------------------------------------
// Fetch weather data from NOAA API (async)
// -----------------------------------------------
pub async fn fetch_weather() -> Result<WeatherRecord> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("building http client")?;

    let station = "KNYC"; // Central Park station (near CCNY)
    let url = format!("https://api.weather.gov/stations/{}/observations/latest", station);

    let resp = client.get(&url)
        .header(
            "User-Agent",
            std::env::var("WEATHER_USER_AGENT")
                .unwrap_or_else(|_| "ccny-weather-bot (your_email@example.com)".into()),
        )
        .send()
        .await
        .context("HTTP request failed")?
        .json::<ObservationResponse>()
        .await
        .context("JSON parse failed")?;

    let temp_c = resp.properties.temperature.and_then(|m| m.value);
    let dew_c = resp.properties.dewpoint.and_then(|m| m.value);
    let humidity = resp.properties.relative_humidity.and_then(|m| m.value);
    let wind_ms = resp.properties.wind_speed.and_then(|m| m.value);
    let wind_dir = resp.properties.wind_direction.and_then(|m| m.value);
    let condition = resp.properties.text_description
        .unwrap_or_else(|| "Unknown".into())
        .chars()
        .take(200)
        .collect::<String>();

    // Unit conversions
    let temp_f = temp_c.map(|c| c * 9.0 / 5.0 + 32.0);
    let dew_f = dew_c.map(|c| c * 9.0 / 5.0 + 32.0);
    let wind_mph = wind_ms.map(|m| m * 2.23694);

    // Format EDT time
    let now_local: DateTime<chrono_tz::Tz> = Utc::now().with_timezone(&New_York);
    let time_str = now_local.format("%b %d, %Y %I:%M %p %Z").to_string();

    Ok(WeatherRecord {
        time: time_str,
        temperature_f: temp_f,
        temperature_c: temp_c,
        dewpoint_f: dew_f,
        dewpoint_c: dew_c,
        humidity,
        wind_speed_mph: wind_mph,
        wind_direction_deg: wind_dir,
        condition,
    })
}

// -----------------------------------------------
// Store a record in SQLite
// -----------------------------------------------
pub fn store_weather(rec: &WeatherRecord) -> Result<()> {
    let conn = open_or_init_db()?;
    conn.execute(
        "INSERT INTO weather (time, temperature_f, temperature_c, dewpoint_f, dewpoint_c, humidity, wind_speed_mph, wind_direction_deg, condition)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            rec.time,
            rec.temperature_f,
            rec.temperature_c,
            rec.dewpoint_f,
            rec.dewpoint_c,
            rec.humidity,
            rec.wind_speed_mph,
            rec.wind_direction_deg,
            rec.condition
        ],
    )?;
    Ok(())
}

// -----------------------------------------------
// Display last 10 weather logs
// -----------------------------------------------
pub fn show_weather_logs() -> Result<()> {
    let conn = open_or_init_db()?;

    let mut stmt = conn.prepare(
        "SELECT time, temperature_f, dewpoint_f, humidity, wind_speed_mph, wind_direction_deg, condition 
         FROM weather ORDER BY id DESC LIMIT 10",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
            row.get::<_, Option<f64>>(3)?,
            row.get::<_, Option<f64>>(4)?,
            row.get::<_, Option<f64>>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;

    println!("\n📊 Recent Weather Logs (latest 10):");
    println!("──────────────────────────────────────────────");

    let mut has_data = false;
    for row in rows {
        has_data = true;
        let (time, temp_f, dew_f, humidity, wind_mph, wind_dir, cond) = row?;
        println!(
        "🕓 {}\n   🌡 Temp: {}°F | 💧 Dew: {}°F | 💦 Humidity: {}% | 💨 Wind: {} mph ({}) | 🌥 {}\n",
        time,
        temp_f.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "N/A".to_string()),
        dew_f.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "N/A".to_string()),
        humidity.map(|v| format!("{:.0}", v)).unwrap_or_else(|| "N/A".to_string()),
        wind_mph.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "N/A".to_string()),
        wind_dir.map(|v| format!("{:.0}°", v)).unwrap_or_else(|| "N/A".to_string()),
        cond);

    }

    if !has_data {
        println!("⚠️  No weather data found. Try fetching it first!");
    }

    println!("──────────────────────────────────────────────\n");
    Ok(())
}
