//! Location-based renewable proxy for Germany.
//!
//! Combines the embedded MaStR installed-capacity grid (`crate::grid`) with
//! Open-Meteo forecast weather (`crate::weather`) to estimate national wind+solar
//! output bottom-up: `Σ_cells capacity × weather-driven capacity factor`. The
//! result is calibrated against the ENTSO-E generation forecast so it lands in
//! real MW and absorbs model/coverage bias, then overlaid on the German chart as
//! an independent cross-check. It is a proxy, not a measurement — ENTSO-E remains
//! the source of truth.

use crate::entsoe::analysis::RenewableSurplus;
use crate::grid::{CapacityCell, DE_CAPACITY_GRID};
use crate::weather::{CellWeather, WeatherClient};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};

/// Performance ratio for the simple PV model (module, inverter, soiling losses).
const SOLAR_PR: f64 = 0.80;

/// Weather is sampled on this coarser grid (degrees) and reused across the fine
/// capacity cells within it. Open-Meteo's free tier weights every location as a
/// call, so a national aggregate must not fetch all ~1400 capacity cells; at ~1°
/// the whole country is a few dozen points, which is ample once calibrated.
const WEATHER_CELL_DEG: f64 = 1.0;

/// Simplified onshore wind power-curve breakpoints (m/s).
const WIND_CUT_IN: f64 = 3.0;
const WIND_RATED: f64 = 12.0;
const WIND_CUT_OUT: f64 = 25.0;

/// One timestamp of modelled output, MW (calibrated by [`calibrate`]).
#[derive(Debug, Clone)]
pub struct ModelledPoint {
    pub timestamp: DateTime<Utc>,
    pub solar_mw: f64,
    pub wind_mw: f64,
    pub total_mw: f64,
}

/// Normalised PV output fraction (0..~1) for a global horizontal irradiance
/// (W/m²) and ambient temperature (°C).
pub fn solar_cf(ghi: f64, temp_c: f64) -> f64 {
    let derate = 1.0 - 0.004 * (temp_c - 25.0).max(0.0);
    ((ghi / 1000.0) * SOLAR_PR * derate).max(0.0)
}

/// Normalised wind turbine output fraction (0..1) for a hub-height wind speed
/// (m/s): zero below cut-in and above cut-out, a cubic ramp to rated, then flat.
pub fn wind_cf(v: f64) -> f64 {
    if !(WIND_CUT_IN..WIND_CUT_OUT).contains(&v) {
        0.0
    } else if v >= WIND_RATED {
        1.0
    } else {
        let num = v.powi(3) - WIND_CUT_IN.powi(3);
        let den = WIND_RATED.powi(3) - WIND_CUT_IN.powi(3);
        (num / den).clamp(0.0, 1.0)
    }
}

/// Re-bin the fine capacity grid onto a coarser grid for weather sampling, so a
/// national aggregate needs only a few dozen Open-Meteo points. Capacity is
/// summed per coarse cell; the centroid is used as the weather point.
fn weather_bins(cells: &[CapacityCell], deg: f64) -> Vec<CapacityCell> {
    let mut bins: BTreeMap<(i64, i64), (f64, f64)> = BTreeMap::new();
    for c in cells {
        let key = ((c.lat / deg).floor() as i64, (c.lon / deg).floor() as i64);
        let entry = bins.entry(key).or_insert((0.0, 0.0));
        entry.0 += c.solar_kw;
        entry.1 += c.wind_kw;
    }
    bins.into_iter()
        .map(|((la, lo), (solar_kw, wind_kw))| CapacityCell {
            lat: (la as f64 + 0.5) * deg,
            lon: (lo as f64 + 0.5) * deg,
            solar_kw,
            wind_kw,
            wind_hub_m: 0.0,
        })
        .collect()
}

/// Combine the capacity grid with per-cell weather into an uncalibrated series.
/// `weather[i]` must correspond to `cells[i]`.
fn build_uncalibrated(cells: &[CapacityCell], weather: &[CellWeather]) -> Vec<ModelledPoint> {
    // timestamp -> (solar_kw, wind_kw)
    let mut acc: BTreeMap<DateTime<Utc>, (f64, f64)> = BTreeMap::new();

    for (cell, cw) in cells.iter().zip(weather.iter()) {
        for h in &cw.hours {
            let entry = acc.entry(h.timestamp).or_insert((0.0, 0.0));
            entry.0 += cell.solar_kw * solar_cf(h.ghi, h.temp_c);
            entry.1 += cell.wind_kw * wind_cf(h.wind_ms);
        }
    }

    acc.into_iter()
        .map(|(timestamp, (solar_kw, wind_kw))| ModelledPoint {
            timestamp,
            solar_mw: solar_kw / 1000.0,
            wind_mw: wind_kw / 1000.0,
            total_mw: (solar_kw + wind_kw) / 1000.0,
        })
        .collect()
}

/// Scale the modelled series so its total matches the ENTSO-E wind+solar
/// generation forecast over the overlapping timestamps. ENTSO-E's surplus series
/// only exposes the combined generation, so a single total scalar is fitted (a
/// least-squares fit through the origin), applied to both components.
pub fn calibrate(mut modelled: Vec<ModelledPoint>, entsoe: &[RenewableSurplus]) -> Vec<ModelledPoint> {
    let truth: HashMap<DateTime<Utc>, f64> =
        entsoe.iter().map(|s| (s.timestamp, s.generation)).collect();

    let (mut sum_model, mut sum_truth) = (0.0, 0.0);
    for m in &modelled {
        if let Some(&g) = truth.get(&m.timestamp) {
            sum_model += m.total_mw;
            sum_truth += g;
        }
    }

    if sum_model > 0.0 && sum_truth > 0.0 {
        let k = sum_truth / sum_model;
        for m in &mut modelled {
            m.solar_mw *= k;
            m.wind_mw *= k;
            m.total_mw *= k;
        }
    }

    modelled
}

/// Compute the calibrated modelled wind+solar series for Germany, or `None` when
/// the capacity grid is empty (no `mastr_ingest` run yet).
pub async fn modelled_de_series(
    weather: &WeatherClient,
    entsoe: &[RenewableSurplus],
) -> anyhow::Result<Option<Vec<ModelledPoint>>> {
    let grid = &*DE_CAPACITY_GRID;
    if grid.is_empty() {
        return Ok(None);
    }

    let bins = weather_bins(&grid.cells, WEATHER_CELL_DEG);
    let points: Vec<(f64, f64)> = bins.iter().map(|c| (c.lat, c.lon)).collect();
    let weather_per_cell = weather.fetch_points(&points).await?;
    let modelled = build_uncalibrated(&bins, &weather_per_cell);
    Ok(Some(calibrate(modelled, entsoe)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::HourlyWeather;

    fn ts(hour: u32) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 6, 24, hour, 0, 0).unwrap()
    }

    #[test]
    fn wind_curve_boundaries() {
        assert_eq!(wind_cf(0.0), 0.0);
        assert_eq!(wind_cf(2.9), 0.0); // below cut-in
        assert_eq!(wind_cf(12.0), 1.0); // rated
        assert_eq!(wind_cf(20.0), 1.0); // plateau
        assert_eq!(wind_cf(24.9), 1.0); // still on plateau
        assert_eq!(wind_cf(25.0), 0.0); // cut-out
        assert_eq!(wind_cf(30.0), 0.0); // above cut-out
        // Monotone, strictly between bounds on the ramp.
        let mid = wind_cf(7.0);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(wind_cf(8.0) > mid);
    }

    #[test]
    fn solar_is_linear_in_irradiance() {
        // At 25°C the derate is 1.0, so output is GHI/1000 * PR.
        assert!((solar_cf(1000.0, 25.0) - SOLAR_PR).abs() < 1e-9);
        assert!((solar_cf(500.0, 25.0) - SOLAR_PR * 0.5).abs() < 1e-9);
        assert_eq!(solar_cf(0.0, 25.0), 0.0);
        // Hotter modules produce slightly less.
        assert!(solar_cf(1000.0, 45.0) < solar_cf(1000.0, 25.0));
    }

    #[test]
    fn calibration_matches_entsoe_total() {
        // One cell, two hours; uncalibrated totals are arbitrary units.
        let cells = vec![CapacityCell {
            lat: 52.0,
            lon: 13.0,
            solar_kw: 1000.0,
            wind_kw: 1000.0,
            wind_hub_m: 100.0,
        }];
        let weather = vec![CellWeather {
            hours: vec![
                HourlyWeather { timestamp: ts(10), ghi: 1000.0, temp_c: 25.0, wind_ms: 12.0 },
                HourlyWeather { timestamp: ts(11), ghi: 500.0, temp_c: 25.0, wind_ms: 12.0 },
            ],
        }];
        let modelled = build_uncalibrated(&cells, &weather);
        assert_eq!(modelled.len(), 2);

        let entsoe = vec![
            RenewableSurplus { timestamp: ts(10), generation: 100.0, load: 0.0, surplus: 0.0 },
            RenewableSurplus { timestamp: ts(11), generation: 50.0, load: 0.0, surplus: 0.0 },
        ];
        let calibrated = calibrate(modelled, &entsoe);
        let total: f64 = calibrated.iter().map(|m| m.total_mw).sum();
        // After calibration the modelled total equals the ENTSO-E total.
        assert!((total - 150.0).abs() < 1e-6);
    }
}
