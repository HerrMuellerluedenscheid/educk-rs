use crate::entsoe::{
    EntsoeClient, EntsoeError, GlMarketDocument, complete_points, finest_resolution,
    merge_forecasts,
};
use crate::i18n::Lang;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};

/// Represents the renewable energy surplus at a point in time
#[derive(Debug, Clone)]
pub struct RenewableSurplus {
    pub timestamp: DateTime<Utc>,
    pub generation: f64,
    pub load: f64,
    pub surplus: f64, // generation - load
    /// Generation per ENTSO-E production type (e.g. "B16" solar), summing to
    /// `generation`.
    pub by_source: BTreeMap<String, f64>,
}

/// A surplus series and whether it may be incomplete.
#[derive(Debug, Clone)]
pub struct SurplusForecast {
    pub series: Vec<RenewableSurplus>,
    /// A generation forecast failed to load (timeout, HTTP error), as opposed
    /// to ENTSO-E having no data for it. The series may then lack production
    /// types it would normally have, e.g. DE onshore wind and solar on a day
    /// where only the intraday forecast has them.
    pub degraded: bool,
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
        self.get_renewable_surplus_series(bidding_zone, period_start, period_end)
            .await?
            .series
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
    ) -> Result<SurplusForecast, EntsoeError> {
        let (day_ahead, intraday, load_forecast) = tokio::join!(
            self.fetch_day_ahead_generation_forecast(bidding_zone, period_start, period_end),
            self.fetch_intraday_generation_forecast(bidding_zone, period_start, period_end),
            self.fetch_day_ahead_total_load_forecast(bidding_zone, period_start, period_end)
        );
        let load_forecast = load_forecast?;

        // Day-ahead first, intraday last so it wins where both have a value.
        // Either one alone is usable; many zones publish no intraday forecast.
        let mut generation = Vec::new();
        let mut degraded = false;
        let mut last_error = None;
        for (process, result) in [("day-ahead", day_ahead), ("intraday", intraday)] {
            match result {
                Ok(doc) => generation.push(doc),
                Err(e) => {
                    if e.is_no_data() {
                        tracing::debug!("no {process} generation forecast for {bidding_zone}");
                    } else {
                        tracing::warn!(
                            "{process} generation forecast for {bidding_zone} failed: {e}"
                        );
                        degraded = true;
                    }
                    last_error = Some(e);
                }
            }
        }
        if generation.is_empty() {
            return Err(last_error.expect("both forecasts failed"));
        }

        Ok(SurplusForecast {
            series: surplus_series(&generation, &load_forecast)?,
            degraded,
        })
    }
}

/// Combine generation forecasts (in increasing order of preference, see
/// [`merge_forecasts`]) and the load forecast into a surplus series. Everything
/// is sampled on the finest grid, so e.g. hourly load still lines up with
/// quarter-hourly generation. Only timestamps present in both are kept.
fn surplus_series(
    gen_forecasts: &[GlMarketDocument],
    load_forecast: &GlMarketDocument,
) -> Result<Vec<RenewableSurplus>, EntsoeError> {
    let step = match (
        finest_resolution(gen_forecasts)?,
        load_forecast.finest_resolution()?,
    ) {
        (Some(g), Some(l)) => g.min(l),
        _ => return Ok(Vec::new()),
    };

    let load_map: HashMap<DateTime<Utc>, f64> = load_forecast
        .aggregated_points(step)?
        .into_iter()
        .map(|p| (p.timestamp, p.quantity))
        .collect();

    let mut surpluses: Vec<RenewableSurplus> =
        complete_points(merge_forecasts(gen_forecasts, step)?)
            .into_iter()
            .filter_map(|gen_point| {
                load_map
                    .get(&gen_point.timestamp)
                    .map(|&load| RenewableSurplus {
                        timestamp: gen_point.timestamp,
                        generation: gen_point.total,
                        load,
                        surplus: gen_point.total - load,
                        by_source: gen_point.by_type,
                    })
            })
            .collect();

    surpluses.sort_by_key(|s| s.timestamp);
    Ok(surpluses)
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
