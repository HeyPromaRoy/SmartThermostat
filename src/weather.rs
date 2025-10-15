use rusqlite::{params, Connection, Result};
use serde::Deserialize;
use chrono::Utc;

#[derive(Deserialize, Debug)]
struct ObservationResponse {
    properties: ObservationProperties,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ObservationProperties {
    temperature: Option<Measurement>,
    wind_speed: Option<Measurement>,
    text_description: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Measurement {
    value: Option<f64>, // Celsius or m/s
}

pub async fn fetch_and_store_weather() -> Result<()> {
    // 1️ Open database (creates if not exists)
    let conn = Connection::open("weather.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS weather (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            time TEXT,
            temperature_f REAL,
            temperature_c REAL,
            wind_speed_mph REAL,
            condition TEXT
        )",
        [],
    )?;

    // NOAA NWS station near CCNY: Central Park (KNYC)
    let client = reqwest::Client::new();
    let station = "KNYC";
    let url = format!("https://api.weather.gov/stations/{}/observations/latest", station);

    // 3️⃣ Fetch observation
    let resp = client
        .get(&url)
        .header("User-Agent", "ccny-weather-bot (your_email@example.com)") // 👈 required by NWS
        .send()
        .await
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
        .json::<ObservationResponse>()
        .await
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    // 4️⃣ Extract data safely
    let temp_c = resp.properties.temperature.and_then(|t| t.value).unwrap_or(f64::NAN);
    let wind_m_s = resp.properties.wind_speed.and_then(|w| w.value).unwrap_or(0.0);
    let condition = resp.properties.text_description.unwrap_or_else(|| "Unknown".into());

    // 5️⃣ Convert units
    let temp_f = temp_c * 9.0 / 5.0 + 32.0;
    let wind_mph = wind_m_s * 2.23694;

    // 6️⃣ Display results
    println!(
        "\n📍 City College of New York (via {})\n🌡 Temp: {:.1}°F / {:.1}°C\n💨 Wind: {:.1} mph\n🌥 Condition: {}\n",
        station, temp_f, temp_c, wind_mph, condition
    );

    // 7️⃣ Save to database
    conn.execute(
        "INSERT INTO weather (time, temperature_f, temperature_c, wind_speed_mph, condition)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Utc::now().to_rfc3339(),
            temp_f,
            temp_c,
            wind_mph,
            condition
        ],
    )?;
    Ok(())
}
