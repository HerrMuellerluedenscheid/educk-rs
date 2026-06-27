//! Open-Meteo forecast client.
//!
//! Fetches the weather variables needed to turn installed capacity into power:
//! global horizontal irradiance (`shortwave_radiation`) for PV, 100 m wind speed
//! for wind, and 2 m temperature for a small PV derate. Free/keyless tier; points
//! are batched comma-separated so a few hundred grid cells cost only a handful of
//! requests. See <https://open-meteo.com/en/docs>.

use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

/// Max points per request. Keeps the comma-separated coordinate URL well within
/// length limits while minimising round-trips.
const BATCH: usize = 100;

/// Hourly forecast weather for a single point.
#[derive(Debug, Clone, Copy)]
pub struct HourlyWeather {
    pub timestamp: DateTime<Utc>,
    /// Global horizontal irradiance, W/m².
    pub ghi: f64,
    /// 2 m air temperature, °C.
    pub temp_c: f64,
    /// Wind speed at 100 m, m/s.
    pub wind_ms: f64,
}

/// The forecast time series for one grid cell.
#[derive(Debug, Clone, Default)]
pub struct CellWeather {
    pub hours: Vec<HourlyWeather>,
}

pub struct WeatherClient {
    client: Client,
}

impl Default for WeatherClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WeatherClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Fetch hourly forecast weather for each `(lat, lon)` point. The returned
    /// vector is in the same order and length as `points`.
    pub async fn fetch_points(&self, points: &[(f64, f64)]) -> Result<Vec<CellWeather>> {
        let mut out = Vec::with_capacity(points.len());

        for chunk in points.chunks(BATCH) {
            let lats = join_coords(chunk.iter().map(|(la, _)| *la));
            let lons = join_coords(chunk.iter().map(|(_, lo)| *lo));

            let url = format!(
                "{FORECAST_URL}?latitude={lats}&longitude={lons}\
                 &hourly=shortwave_radiation,temperature_2m,wind_speed_100m\
                 &wind_speed_unit=ms&forecast_days=2&timezone=UTC"
            );

            let resp = self.client.get(&url).send().await?;
            let status = resp.status();
            let body = resp.text().await?;
            if !status.is_success() {
                // e.g. HTTP 429 when too many locations are requested on the free
                // tier; the body carries Open-Meteo's "reason".
                let snippet: String = body.chars().take(200).collect();
                anyhow::bail!("Open-Meteo returned {status}: {snippet}");
            }
            let value: serde_json::Value = serde_json::from_str(&body)?;

            // Open-Meteo returns a bare object for one point and an array for many.
            let responses: Vec<OmResponse> = match value {
                serde_json::Value::Array(_) => serde_json::from_value(value)?,
                _ => vec![serde_json::from_value(value)?],
            };

            for r in responses {
                out.push(r.into_cell_weather());
            }
        }

        Ok(out)
    }
}

fn join_coords(it: impl Iterator<Item = f64>) -> String {
    it.map(|c| format!("{c:.3}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Deserialize)]
struct OmResponse {
    hourly: OmHourly,
}

#[derive(Deserialize)]
struct OmHourly {
    time: Vec<String>,
    #[serde(default)]
    shortwave_radiation: Vec<Option<f64>>,
    #[serde(default)]
    temperature_2m: Vec<Option<f64>>,
    #[serde(default)]
    wind_speed_100m: Vec<Option<f64>>,
}

impl OmResponse {
    fn into_cell_weather(self) -> CellWeather {
        let h = self.hourly;
        let mut hours = Vec::with_capacity(h.time.len());

        for (i, t) in h.time.iter().enumerate() {
            let Some(timestamp) = parse_om_time(t) else {
                continue;
            };
            hours.push(HourlyWeather {
                timestamp,
                ghi: h.shortwave_radiation.get(i).copied().flatten().unwrap_or(0.0),
                temp_c: h.temperature_2m.get(i).copied().flatten().unwrap_or(15.0),
                wind_ms: h.wind_speed_100m.get(i).copied().flatten().unwrap_or(0.0),
            });
        }

        CellWeather { hours }
    }
}

/// Parse Open-Meteo's `YYYY-MM-DDTHH:MM` timestamps (already UTC when requested
/// with `timezone=UTC`).
fn parse_om_time(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .ok()
        .map(|ndt| DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo_timestamp() {
        let ts = parse_om_time("2026-06-24T13:00").unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-06-24T13:00:00+00:00");
    }

    #[test]
    fn parses_array_response_into_cells() {
        let body = r#"[
            {"latitude":52.5,"longitude":13.4,"hourly":{
                "time":["2026-06-24T00:00","2026-06-24T01:00"],
                "shortwave_radiation":[0.0,120.5],
                "temperature_2m":[14.0,15.0],
                "wind_speed_100m":[6.0,7.5]}},
            {"latitude":48.1,"longitude":11.6,"hourly":{
                "time":["2026-06-24T00:00"],
                "shortwave_radiation":[null],
                "temperature_2m":[null],
                "wind_speed_100m":[null]}}
        ]"#;
        let value: serde_json::Value = serde_json::from_str(body).unwrap();
        let responses: Vec<OmResponse> = serde_json::from_value(value).unwrap();
        let cells: Vec<CellWeather> = responses.into_iter().map(|r| r.into_cell_weather()).collect();

        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].hours.len(), 2);
        assert_eq!(cells[0].hours[1].ghi, 120.5);
        assert_eq!(cells[0].hours[1].wind_ms, 7.5);
        // Missing values fall back to sane defaults.
        assert_eq!(cells[1].hours[0].ghi, 0.0);
        assert_eq!(cells[1].hours[0].temp_c, 15.0);
    }
}
