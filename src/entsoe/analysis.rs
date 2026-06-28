use crate::entsoe::{EntsoeClient, EntsoeError};
use crate::i18n::Lang;
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
        let surpluses: Vec<RenewableSurplus> = gen_points
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

    /// Share of electricity demand (load) covered by wind + solar generation,
    /// as a percentage. Can exceed 100% when there is a renewable surplus.
    pub fn renewable_share(&self) -> f64 {
        if self.load == 0.0 {
            0.0
        } else {
            (self.generation / self.load) * 100.0
        }
    }

    /// Check if there's excess renewable energy (generation > load)
    pub fn has_excess(&self) -> bool {
        self.surplus > 0.0
    }
}

/// A human-readable, SEO-friendly summary derived from a renewable surplus
/// forecast series. All times are UTC.
#[derive(Debug, Clone)]
pub struct ForecastSummary {
    /// Ready-to-render descriptive sentences.
    pub sentences: Vec<String>,
}

impl ForecastSummary {
    /// A single sentence suitable for a `<meta name="description">`, truncated to
    /// a search-engine-friendly length.
    pub fn meta_description(&self, country_name: &str, lang: Lang) -> String {
        let body = self.sentences.first().cloned().unwrap_or_else(|| match lang {
            Lang::En => format!("Renewable electricity forecast for {country_name}."),
            Lang::De => format!("Erneuerbare-Energien-Prognose für {country_name}."),
        });
        truncate_on_word_boundary(&body, 155)
    }
}

/// Build a descriptive summary from a forecast series. `country_name` is the
/// (already localized) country name used in the generated prose, e.g. "Belgium" /
/// "Belgien"; `lang` selects the language of the sentences.
pub fn summarize_forecast(
    series: &[RenewableSurplus],
    country_name: &str,
    lang: Lang,
) -> ForecastSummary {
    if series.is_empty() {
        return ForecastSummary {
            sentences: vec![match lang {
                Lang::En => format!(
                    "No renewable energy forecast is currently available for {country_name}."
                ),
                Lang::De => {
                    format!("Für {country_name} ist derzeit keine Erneuerbare-Energien-Prognose verfügbar.")
                }
            }],
        };
    }

    let now_ts = Utc::now();
    let now = series
        .iter()
        .find(|s| s.timestamp >= now_ts)
        .or_else(|| series.last())
        .cloned();

    let cmp_share = |a: &&RenewableSurplus, b: &&RenewableSurplus| {
        a.renewable_share()
            .partial_cmp(&b.renewable_share())
            .unwrap_or(std::cmp::Ordering::Equal)
    };
    let peak = series.iter().max_by(cmp_share).cloned();
    let trough = series.iter().min_by(cmp_share).cloned();
    let best_window = series
        .iter()
        .max_by(|a, b| {
            a.surplus
                .partial_cmp(&b.surplus)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned();

    let mut sentences = Vec::new();
    let hm = |s: &RenewableSurplus| s.timestamp.format("%H:%M").to_string();

    if let Some(n) = &now {
        sentences.push(match lang {
            Lang::En => format!(
                "Wind and solar are forecast to cover about {:.0}% of {}'s electricity demand around {} UTC.",
                n.renewable_share(), country_name, hm(n),
            ),
            Lang::De => format!(
                "Wind und Sonne decken gegen {} UTC voraussichtlich rund {:.0}% des Strombedarfs von {}.",
                hm(n), n.renewable_share(), country_name,
            ),
        });
    }
    if let Some(p) = &peak {
        sentences.push(match lang {
            Lang::En => format!(
                "Renewable output peaks around {} UTC at roughly {:.0}% of demand — the greenest time to use electricity today.",
                hm(p), p.renewable_share(),
            ),
            Lang::De => format!(
                "Die Erneuerbaren-Erzeugung erreicht gegen {} UTC ihren Höchstwert von etwa {:.0}% des Bedarfs — die grünste Zeit, um heute Strom zu nutzen.",
                hm(p), p.renewable_share(),
            ),
        });
    }
    if let Some(t) = &trough {
        sentences.push(match lang {
            Lang::En => format!(
                "The renewable share dips to about {:.0}% around {} UTC, when grid electricity is at its most carbon-intensive.",
                t.renewable_share(), hm(t),
            ),
            Lang::De => format!(
                "Der Erneuerbaren-Anteil sinkt gegen {} UTC auf etwa {:.0}%, wenn der Netzstrom am CO₂-intensivsten ist.",
                hm(t), t.renewable_share(),
            ),
        });
    }
    if let Some(b) = &best_window {
        if b.has_excess() {
            sentences.push(match lang {
                Lang::En => format!(
                    "There is forecast surplus renewable generation around {} UTC — an ideal low-carbon window for flexible loads such as EV charging, laundry or heating.",
                    hm(b),
                ),
                Lang::De => format!(
                    "Gegen {} UTC wird ein Überschuss an erneuerbarer Erzeugung erwartet — ein ideales CO₂-armes Zeitfenster für flexible Lasten wie E-Auto-Laden, Wäsche oder Heizen.",
                    hm(b),
                ),
            });
        }
    }

    ForecastSummary { sentences }
}

/// Truncate `s` to at most `max` chars, breaking on a word boundary and adding an
/// ellipsis when shortened.
fn truncate_on_word_boundary(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max - 1).collect();
    let cut = match truncated.rfind(' ') {
        Some(idx) => &truncated[..idx],
        None => truncated.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', '.', ' ']))
}
