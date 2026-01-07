use crate::entsoe::{EntsoeClient, EntsoeError};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Represents the renewable energy surplus at a point in time
#[derive(Debug, Clone)]
pub struct RenewableSurplus {
    pub timestamp: DateTime<Utc>,
    pub generation: f64,
    pub load: f64,
    pub surplus: f64, // generation - load
}

impl EntsoeClient {
    /// Find the time with maximum renewable energy surplus (generation - load)
    /// Returns the timestamp and values when renewable surplus is highest
    pub async fn find_max_renewable_surplus(
        &self,
        bidding_zone: &str,
        period_start: &str,
        period_end: &str,
    ) -> Result<RenewableSurplus, EntsoeError> {
        // Fetch both forecasts in parallel
        let (gen_forecast, load_forecast) = tokio::try_join!(
            self.fetch_day_ahead_generation_forecast(bidding_zone, period_start, period_end),
            self.fetch_day_ahead_total_load_forecast(bidding_zone, period_start, period_end)
        )?;

        // Get timestamped points for both
        let gen_points = gen_forecast.all_timestamped_points()?;
        let load_points = load_forecast.all_timestamped_points()?;

        // Create a map of load by timestamp for quick lookup
        let load_map: HashMap<DateTime<Utc>, f64> = load_points
            .into_iter()
            .map(|p| (p.timestamp, p.quantity))
            .collect();

        // Calculate surplus for each generation point that has matching load data
        let mut surpluses: Vec<RenewableSurplus> = gen_points
            .into_iter()
            .filter_map(|gen_point| {
                load_map
                    .get(&gen_point.timestamp)
                    .map(|&load| RenewableSurplus {
                        timestamp: gen_point.timestamp,
                        generation: gen_point.quantity,
                        load,
                        surplus: gen_point.quantity - load,
                    })
            })
            .collect();

        // Find the maximum surplus
        surpluses
            .into_iter()
            .max_by(|a, b| {
                a.surplus
                    .partial_cmp(&b.surplus)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or(EntsoeError::InvalidResponse(
                "No matching data points found".to_string(),
            ))
    }

    /// Get all renewable surplus data points for analysis
    pub async fn get_renewable_surplus_series(
        &self,
        bidding_zone: &str,
        period_start: &str,
        period_end: &str,
    ) -> Result<Vec<RenewableSurplus>, EntsoeError> {
        // Fetch both forecasts in parallel
        let (gen_forecast, load_forecast) = tokio::try_join!(
            self.fetch_day_ahead_generation_forecast(bidding_zone, period_start, period_end),
            self.fetch_day_ahead_total_load_forecast(bidding_zone, period_start, period_end)
        )?;

        // Get timestamped points for both
        let gen_points = gen_forecast.all_timestamped_points()?;
        let load_points = load_forecast.all_timestamped_points()?;

        // Create a map of load by timestamp
        let load_map: HashMap<DateTime<Utc>, f64> = load_points
            .into_iter()
            .map(|p| (p.timestamp, p.quantity))
            .collect();

        // Calculate surplus for all points
        let mut surpluses: Vec<RenewableSurplus> = gen_points
            .into_iter()
            .filter_map(|gen_point| {
                load_map
                    .get(&gen_point.timestamp)
                    .map(|&load| RenewableSurplus {
                        timestamp: gen_point.timestamp,
                        generation: gen_point.quantity,
                        load,
                        surplus: gen_point.quantity - load,
                    })
            })
            .collect();

        // Sort by timestamp
        surpluses.sort_by_key(|s| s.timestamp);

        Ok(surpluses)
    }
}

impl RenewableSurplus {
    /// Calculate the surplus as a percentage of generation
    pub fn surplus_percentage(&self) -> f64 {
        if self.generation == 0.0 {
            0.0
        } else {
            (self.surplus / self.generation) * 100.0
        }
    }

    /// Check if there's excess renewable energy (generation > load)
    pub fn has_excess(&self) -> bool {
        self.surplus > 0.0
    }
}
