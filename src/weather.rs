use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use chrono_tz::America::New_York;
use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde::Deserialize;
use rusqlite::Connection;
use crate::db;

#[derive(Debug, Deserialize)]
pub struct ObservationResponse {
    pub properties: ObservationProperties,
}

#[derive(Debug, Deserialize)]
pub struct ObservationProperties {
    pub textDescription: Option<String>,
    pub temperature: Option<Measurement>,
    pub dewpoint: Option<Measurement>,
    pub relativeHumidity: Option<Measurement>,
    pub windSpeed: Option<Measurement>,
    pub windDirection: Option<Measurement>,
}

#[derive(Debug, Deserialize)]
pub struct Measurement {
    pub value: Option<f64>,
}

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

pub fn fetch_weather() -> Result<WeatherRecord> {

    let station = "KNYC"; // Central Park station (near CCNY)
    let url = format!("https://api.weather.gov/stations/{}/observations/latest", station);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(Policy::none())
        .build()
        .context("building http client")?;

    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            std::env::var("WEATHER_USER_AGENT")
                .unwrap_or_else(|_| "ccny-weather-bot (your_email@example.com)".into()),
        )
        .send()
        .context("HTTP request failed")?
        .json::<ObservationResponse>()
        .context("JSON parse failed")?;

    let temp_c = resp.properties.temperature.and_then(|m| m.value);
    let dew_c = resp.properties.dewpoint.and_then(|m| m.value);
    let humidity = resp.properties.relativeHumidity.and_then(|m| m.value);
    let wind_ms = resp.properties.windSpeed.and_then(|m| m.value);
    let wind_dir = resp.properties.windDirection.and_then(|m| m.value);
    let condition = resp
        .properties
        .textDescription
        .unwrap_or_else(|| "Unknown".into())
        .chars()
        .take(200)
        .collect::<String>();

    let temp_f = temp_c.map(|c| c * 9.0 / 5.0 + 32.0);
    let dew_f = dew_c.map(|c| c * 9.0 / 5.0 + 32.0);
    let wind_mph = wind_ms.map(|m| m * 2.23694);

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

pub fn get_current_weather(conn: &mut Connection) -> Result<()> {
    let data = fetch_weather()?;

    println!("🌈✨=============================================✨🌈");
    println!("🌤️  Condition: {}", data.condition);
    println!(
        "🌡️  Temperature: {:.1}°F / {:.1}°C",
        data.temperature_f.unwrap_or(0.0),
        data.temperature_c.unwrap_or(0.0)
    );
    println!(
        "💧  Dewpoint: {:.1}°F / {:.1}°C",
        data.dewpoint_f.unwrap_or(0.0),
        data.dewpoint_c.unwrap_or(0.0)
    );
    println!("💦  Humidity: {:.1}%", data.humidity.unwrap_or(0.0));
    println!("💨  Wind Speed: {:.1} mph", data.wind_speed_mph.unwrap_or(0.0));
    println!("🧭  Wind Direction: {:.1}°", data.wind_direction_deg.unwrap_or(0.0));
    println!("🕒  Time: {}", data.time);
    println!("🌈✨=============================================✨🌈");

    db::insert_weather(conn, &data)?;
    Ok(())
}