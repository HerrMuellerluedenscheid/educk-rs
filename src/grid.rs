//! Embedded German installed-capacity grid.
//!
//! Produced offline by the `mastr_ingest` tool from the MaStR Gesamtdatenexport
//! (see `src/bin/mastr_ingest.rs`). Installed solar/wind capacity is binned into
//! a coarse lat/lon grid so the runtime only needs one weather point per cell.
//! While the grid is empty (no ingest run yet) the modelled overlay is skipped.

use once_cell::sync::Lazy;
use serde::Deserialize;

/// A spatial bin of installed renewable capacity.
#[derive(Debug, Clone, Deserialize)]
pub struct CapacityCell {
    pub lat: f64,
    pub lon: f64,
    /// Installed gross solar (PV) capacity in this cell, kW.
    pub solar_kw: f64,
    /// Installed gross wind capacity in this cell, kW.
    pub wind_kw: f64,
    /// Capacity-weighted mean wind hub height in this cell, metres (0 if unknown).
    #[serde(default)]
    pub wind_hub_m: f64,
}

/// The full grid plus provenance metadata.
#[derive(Debug, Clone, Deserialize)]
pub struct CapacityGrid {
    /// ISO date the source export was generated, or `"seed"` for the placeholder.
    pub generated: String,
    /// Cell edge length in degrees.
    pub cell_deg: f64,
    /// Fraction of national solar capacity that carried usable coordinates.
    #[serde(default)]
    pub solar_capture_frac: f64,
    /// Fraction of national wind capacity that carried usable coordinates.
    #[serde(default)]
    pub wind_capture_frac: f64,
    pub cells: Vec<CapacityCell>,
}

impl CapacityGrid {
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn total_solar_kw(&self) -> f64 {
        self.cells.iter().map(|c| c.solar_kw).sum()
    }

    pub fn total_wind_kw(&self) -> f64 {
        self.cells.iter().map(|c| c.wind_kw).sum()
    }
}

const DE_GRID_JSON: &str = include_str!("../static/de_capacity_grid.json");

/// The German installed-capacity grid, parsed once at startup.
pub static DE_CAPACITY_GRID: Lazy<CapacityGrid> = Lazy::new(|| {
    serde_json::from_str(DE_GRID_JSON).expect("static/de_capacity_grid.json is valid JSON")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_grid_parses() {
        // The committed JSON must always deserialize, or the server panics on
        // first request. Totals must be consistent (non-negative).
        let grid = &*DE_CAPACITY_GRID;
        assert!(grid.cell_deg > 0.0);
        assert!(grid.total_solar_kw() >= 0.0);
        assert!(grid.total_wind_kw() >= 0.0);
    }
}
