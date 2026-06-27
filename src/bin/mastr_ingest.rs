//! Offline ingest of the MaStR Gesamtdatenexport into a compact capacity grid.
//!
//! Reads the ~3 GB ZIP export from the Marktstammdatenregister
//! (<https://www.marktstammdatenregister.de/MaStR/Datendownload>), streams the
//! solar and wind unit XML files, keeps in-operation units that carry
//! coordinates, bins their installed gross capacity into a coarse lat/lon grid,
//! and writes `static/de_capacity_grid.json` for the server to embed.
//!
//! Usage:
//!   cargo run --release --bin mastr_ingest -- /path/to/Gesamtdatenexport.zip [out.json]
//!
//! Data licensed under "Datenlizenz Deutschland – Namensnennung – Version 2.0".
//!
//! This binary is intentionally self-contained: its only contract with the
//! server is the JSON schema it writes (mirrored by `crate::grid::CapacityGrid`).

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, bail};
use encoding_rs_io::DecodeReaderBytes;
use quick_xml::Reader;
use quick_xml::events::Event;

/// Grid cell edge length in degrees (~20 km in latitude).
const CELL_DEG: f64 = 0.2;

/// MaStR operating-status code for "In Betrieb" (in operation).
const STATUS_IN_BETRIEB: &str = "35";

/// Rough bounding box for Germany (incl. offshore), to drop bogus coordinates.
const LAT_MIN: f64 = 47.0;
const LAT_MAX: f64 = 56.5;
const LON_MIN: f64 = 5.0;
const LON_MAX: f64 = 16.0;

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Solar,
    Wind,
}

/// Accumulator for one grid cell (kW).
#[derive(Default)]
struct CellAcc {
    solar_kw: f64,
    wind_kw: f64,
    /// Σ hub_height × wind_kw, for a capacity-weighted mean.
    hub_weighted: f64,
    hub_cap: f64,
}

#[derive(Default)]
struct Stats {
    total_solar_kw: f64,
    total_wind_kw: f64,
    captured_solar_kw: f64,
    captured_wind_kw: f64,
    solar_units: u64,
    wind_units: u64,
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let zip_path = args
        .next()
        .context("usage: mastr_ingest <Gesamtdatenexport.zip> [out.json]")?;
    let out_path = args
        .next()
        .unwrap_or_else(|| "static/de_capacity_grid.json".to_string());

    let file = File::open(&zip_path).with_context(|| format!("opening {zip_path}"))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file)).context("reading ZIP archive")?;

    let mut cells: HashMap<(i64, i64), CellAcc> = HashMap::new();
    let mut stats = Stats::default();

    for i in 0..archive.len() {
        // Borrow the entry just long enough to learn its name.
        let name = archive.by_index(i)?.name().to_string();
        let lower = name.to_ascii_lowercase();
        let source = if !lower.ends_with(".xml") {
            continue
        } else if lower.contains("einheitensolar") {
            Source::Solar
        } else if lower.contains("einheitenwind") {
            Source::Wind
        } else {
            continue;
        };

        eprintln!("parsing {name} ...");
        let entry = archive.by_index(i)?;
        // The export declares UTF-16; transcode to UTF-8 for quick-xml.
        let decoded = DecodeReaderBytes::new(entry);
        parse_units(BufReader::new(decoded), source, &mut cells, &mut stats)
            .with_context(|| format!("parsing {name}"))?;
    }

    if cells.is_empty() {
        bail!(
            "no usable solar/wind units found — is this a MaStR Gesamtdatenexport ZIP? \
             (expected EinheitenSolar*.xml / EinheitenWind*.xml entries)"
        );
    }

    write_grid(&out_path, &cells, &stats)?;
    report(&stats, cells.len(), &out_path);
    Ok(())
}

/// Stream-parse one unit XML file, accumulating capacity per grid cell.
///
/// Units are the depth-2 elements (children of the file root); their fields are
/// the depth-3 elements. The exact unit element name is not assumed.
fn parse_units<R: std::io::BufRead>(
    reader: R,
    source: Source,
    cells: &mut HashMap<(i64, i64), CellAcc>,
    stats: &mut Stats,
) -> Result<()> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut depth: i32 = 0;
    let mut field: Option<Field> = None;
    let mut rec = Record::default();

    loop {
        match xml.read_event_into(&mut buf)? {
            Event::Start(e) => {
                depth += 1;
                if depth == 2 {
                    rec = Record::default(); // new unit
                } else if depth == 3 {
                    field = Field::from_local(e.local_name().as_ref());
                }
            }
            Event::Text(t) => {
                if depth == 3 && let Some(f) = field {
                    let text = t.xml_content()?.trim().to_string();
                    if !text.is_empty() {
                        rec.set(f, text);
                    }
                }
            }
            Event::End(_) => {
                if depth == 3 {
                    field = None;
                } else if depth == 2 {
                    rec.finalize(source, cells, stats);
                }
                depth -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum Field {
    Status,
    Lat,
    Lon,
    Brutto,
    Hub,
}

impl Field {
    fn from_local(name: &[u8]) -> Option<Field> {
        match name {
            b"EinheitBetriebsstatus" => Some(Field::Status),
            b"Breitengrad" => Some(Field::Lat),
            b"Laengengrad" => Some(Field::Lon),
            b"Bruttoleistung" => Some(Field::Brutto),
            b"Nabenhoehe" => Some(Field::Hub),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Record {
    status: Option<String>,
    lat: Option<String>,
    lon: Option<String>,
    brutto: Option<String>,
    hub: Option<String>,
}

impl Record {
    fn set(&mut self, field: Field, value: String) {
        match field {
            Field::Status => self.status = Some(value),
            Field::Lat => self.lat = Some(value),
            Field::Lon => self.lon = Some(value),
            Field::Brutto => self.brutto = Some(value),
            Field::Hub => self.hub = Some(value),
        }
    }

    fn finalize(
        &self,
        source: Source,
        cells: &mut HashMap<(i64, i64), CellAcc>,
        stats: &mut Stats,
    ) {
        // Only count units that are actually in operation.
        if self.status.as_deref() != Some(STATUS_IN_BETRIEB) {
            return;
        }
        let Some(kw) = self.brutto.as_deref().and_then(parse_de_number) else {
            return;
        };
        if kw <= 0.0 {
            return;
        }

        match source {
            Source::Solar => {
                stats.total_solar_kw += kw;
                stats.solar_units += 1;
            }
            Source::Wind => {
                stats.total_wind_kw += kw;
                stats.wind_units += 1;
            }
        }

        // Needs usable coordinates to be placed on the grid (small rooftop PV is
        // frequently suppressed — those units count toward totals but not capture).
        let (Some(lat), Some(lon)) = (
            self.lat.as_deref().and_then(parse_de_number),
            self.lon.as_deref().and_then(parse_de_number),
        ) else {
            return;
        };
        if !(LAT_MIN..=LAT_MAX).contains(&lat) || !(LON_MIN..=LON_MAX).contains(&lon) {
            return;
        }

        let key = ((lat / CELL_DEG).floor() as i64, (lon / CELL_DEG).floor() as i64);
        let cell = cells.entry(key).or_default();
        match source {
            Source::Solar => {
                cell.solar_kw += kw;
                stats.captured_solar_kw += kw;
            }
            Source::Wind => {
                cell.wind_kw += kw;
                stats.captured_wind_kw += kw;
                if let Some(hub) = self.hub.as_deref().and_then(parse_de_number)
                    && hub > 0.0
                {
                    cell.hub_weighted += hub * kw;
                    cell.hub_cap += kw;
                }
            }
        }
    }
}

/// Parse a MaStR numeric string, tolerating either decimal separator.
fn parse_de_number(s: &str) -> Option<f64> {
    s.replace(',', ".").parse::<f64>().ok()
}

fn write_grid(out_path: &str, cells: &HashMap<(i64, i64), CellAcc>, stats: &Stats) -> Result<()> {
    let mut json_cells: Vec<serde_json::Value> = Vec::with_capacity(cells.len());
    let mut keys: Vec<&(i64, i64)> = cells.keys().collect();
    keys.sort(); // deterministic output

    for key in keys {
        let acc = &cells[key];
        // Cell centroid.
        let lat = (key.0 as f64 + 0.5) * CELL_DEG;
        let lon = (key.1 as f64 + 0.5) * CELL_DEG;
        let hub = if acc.hub_cap > 0.0 {
            acc.hub_weighted / acc.hub_cap
        } else {
            0.0
        };
        json_cells.push(serde_json::json!({
            "lat": round(lat, 3),
            "lon": round(lon, 3),
            "solar_kw": round(acc.solar_kw, 1),
            "wind_kw": round(acc.wind_kw, 1),
            "wind_hub_m": round(hub, 1),
        }));
    }

    let frac = |captured: f64, total: f64| if total > 0.0 { captured / total } else { 0.0 };
    let doc = serde_json::json!({
        "generated": today(),
        "cell_deg": CELL_DEG,
        "solar_capture_frac": round(frac(stats.captured_solar_kw, stats.total_solar_kw), 4),
        "wind_capture_frac": round(frac(stats.captured_wind_kw, stats.total_wind_kw), 4),
        "cells": json_cells,
    });

    if let Some(parent) = Path::new(out_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(out_path, serde_json::to_string_pretty(&doc)?)
        .with_context(|| format!("writing {out_path}"))?;
    Ok(())
}

fn report(stats: &Stats, n_cells: usize, out_path: &str) {
    let gw = |kw: f64| kw / 1.0e6;
    eprintln!("\n── MaStR ingest summary ──────────────────────────────");
    eprintln!(
        "solar: {:>7} units, {:6.1} GW in operation, {:6.1} GW geolocated ({:.0}% captured)",
        stats.solar_units,
        gw(stats.total_solar_kw),
        gw(stats.captured_solar_kw),
        100.0 * safe_frac(stats.captured_solar_kw, stats.total_solar_kw),
    );
    eprintln!(
        "wind:  {:>7} units, {:6.1} GW in operation, {:6.1} GW geolocated ({:.0}% captured)",
        stats.wind_units,
        gw(stats.total_wind_kw),
        gw(stats.captured_wind_kw),
        100.0 * safe_frac(stats.captured_wind_kw, stats.total_wind_kw),
    );
    eprintln!("grid:  {n_cells} cells @ {CELL_DEG}° → {out_path}");
    eprintln!("───────────────────────────────────────────────────────");
}

fn safe_frac(a: f64, b: f64) -> f64 {
    if b > 0.0 { a / b } else { 0.0 }
}

fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

/// Today's date as `YYYY-MM-DD` (UTC), without pulling in chrono here.
fn today() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
