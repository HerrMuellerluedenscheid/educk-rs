use axum::{
    Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Json},
    routing::get,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::cloud;
use crate::config::Config;
use crate::entsoe::analysis::{RenewableSurplus, summarize_forecast};
use crate::entsoe::areas::get_primary_zone;
use crate::entsoe::{EntsoeClient, areas};

/// How far ahead the SSR pages forecast, and how long a fetched series is reused.
const LOOKAHEAD_HOURS: i64 = 25;
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

type SeriesCache = Arc<Mutex<HashMap<String, (Instant, Vec<RenewableSurplus>)>>>;

#[derive(Clone)]
struct AppState {
    entsoe_client: Arc<EntsoeClient>,
    /// Public origin for canonical URLs / Open Graph tags on SSR pages.
    base_url: Arc<str>,
    /// Per-zone forecast cache so the SSR pages (esp. the apex landing) don't
    /// hit ENTSO-E on every request. Day-ahead data changes slowly.
    series_cache: SeriesCache,
}

impl AppState {
    /// Fetch the next ~25h renewable surplus series for a bidding zone, reusing a
    /// cached result younger than `CACHE_TTL`.
    async fn series_for_zone(&self, zone_code: &str) -> Result<Vec<RenewableSurplus>, StatusCode> {
        {
            let cache = self.series_cache.lock().unwrap();
            if let Some((fetched_at, data)) = cache.get(zone_code) {
                if fetched_at.elapsed() < CACHE_TTL {
                    return Ok(data.clone());
                }
            }
        } // drop the guard before awaiting

        let now = Utc::now();
        let end = now + Duration::hours(LOOKAHEAD_HOURS);
        let (period_start, period_end) = format_period(now, end);

        let series = self
            .entsoe_client
            .get_renewable_surplus_series(zone_code, &period_start, &period_end)
            .await
            .map_err(|e| {
                tracing::error!("ENTSO-E API error: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        self.series_cache
            .lock()
            .unwrap()
            .insert(zone_code.to_string(), (Instant::now(), series.clone()));
        Ok(series)
    }
}

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}

#[derive(Serialize)]
struct MaxSurplusResponse {
    country_code: String,
    timestamp: String,
    timestamp_utc: String,
    generation_mw: f64,
    load_mw: f64,
    surplus_mw: f64,
    surplus_percentage: f64,
    renewable_penetration: f64,
    filter_applied: String,
}

impl From<RenewableSurplus> for MaxSurplusResponse {
    fn from(surplus: RenewableSurplus) -> Self {
        Self {
            country_code: String::new(), // Will be set later
            timestamp: surplus.timestamp.to_rfc3339(),
            timestamp_utc: surplus
                .timestamp
                .format("%Y-%m-%d %H:%M:%S UTC")
                .to_string(),
            generation_mw: surplus.generation,
            load_mw: surplus.load,
            surplus_mw: surplus.surplus,
            surplus_percentage: surplus.surplus_percentage(),
            // renewable_penetration: surplus.renewable_penetration(),
            renewable_penetration: 0.0,    // todo fix
            filter_applied: String::new(), // Will be set later
        }
    }
}

#[derive(Deserialize)]
struct TimeQuery {
    /// Number of hours to look ahead (default: 24)
    hours: Option<u32>,
}

/// Filter surplus data to only night hours (22:00-06:00)
fn filter_night_hours(series: Vec<RenewableSurplus>) -> Vec<RenewableSurplus> {
    series
        .into_iter()
        .filter(|s| {
            let hour = s.timestamp.hour();
            hour >= 22 || hour < 6
        })
        .collect()
}

/// Filter surplus data to only the next N hours from now
fn filter_next_hours(series: Vec<RenewableSurplus>, hours: u32) -> Vec<RenewableSurplus> {
    let now = Utc::now();
    let end_time = now + Duration::hours(hours as i64);

    series
        .into_iter()
        .filter(|s| s.timestamp >= now && s.timestamp <= end_time)
        .collect()
}

/// Find maximum surplus in a series
fn find_max(series: Vec<RenewableSurplus>) -> Option<RenewableSurplus> {
    series.into_iter().max_by(|a, b| {
        a.surplus
            .partial_cmp(&b.surplus)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Format period times for ENTSO-E API (YYYYMMDDHHmm)
fn format_period(start: DateTime<Utc>, end: DateTime<Utc>) -> (String, String) {
    (
        start.format("%Y%m%d%H%M").to_string(),
        end.format("%Y%m%d%H%M").to_string(),
    )
}

/// GET /api/v1/renewable-surplus/:country/night
/// Find maximum renewable surplus during night hours (22:00-06:00)
async fn get_night_surplus(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
) -> Result<Json<ApiResponse<MaxSurplusResponse>>, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::BAD_REQUEST)?;

    let now = Utc::now();
    let end = now + Duration::hours(48); // Look ahead 48 hours to ensure we have night hours
    let (period_start, period_end) = format_period(now, end);

    let series = state
        .entsoe_client
        .get_renewable_surplus_series(zone.code, &period_start, &period_end)
        .await
        .map_err(|e| {
            tracing::error!("ENTSO-E API error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let night_series = filter_night_hours(series);

    if let Some(max_surplus) = find_max(night_series) {
        let mut response: MaxSurplusResponse = max_surplus.into();
        response.country_code = country_code.parse().unwrap();
        response.filter_applied = "Night hours (22:00-06:00)".to_string();

        Ok(Json(ApiResponse::success(response)))
    } else {
        Ok(Json(ApiResponse::error(
            "No night hours found in forecast period".to_string(),
        )))
    }
}

/// GET /api/v1/renewable-surplus/:country/next-6h
/// Find maximum renewable surplus within the next 6 hours
async fn get_next_6h_surplus(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
) -> Result<Json<ApiResponse<MaxSurplusResponse>>, StatusCode> {
    get_next_hours_surplus(state, &country_code, 6).await
}

/// GET /api/v1/renewable-surplus/:country/next-24h
/// Find maximum renewable surplus within the next 24 hours
async fn get_next_24h_surplus(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
) -> Result<Json<ApiResponse<MaxSurplusResponse>>, StatusCode> {
    get_next_hours_surplus(state, &country_code, 24).await
}

/// GET /api/v1/renewable-surplus/:country/next?hours=N
/// Find maximum renewable surplus within the next N hours (custom)
async fn get_custom_hours_surplus(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
    Query(query): Query<TimeQuery>,
) -> Result<Json<ApiResponse<MaxSurplusResponse>>, StatusCode> {
    let hours = query.hours.unwrap_or(24);
    get_next_hours_surplus(state, &country_code, hours).await
}

/// Helper function to get surplus for next N hours
async fn get_next_hours_surplus(
    state: AppState,
    country_code: &str,
    hours: u32,
) -> Result<Json<ApiResponse<MaxSurplusResponse>>, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::BAD_REQUEST)?;

    let now = Utc::now();
    let end = now + Duration::hours((hours + 1) as i64); // Add 1 hour buffer
    let (period_start, period_end) = format_period(now, end);

    let series = state
        .entsoe_client
        .get_renewable_surplus_series(zone.code, &period_start, &period_end)
        .await
        .map_err(|e| {
            tracing::error!("ENTSO-E API error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let filtered_series = filter_next_hours(series, hours);

    if let Some(max_surplus) = find_max(filtered_series) {
        let mut response: MaxSurplusResponse = max_surplus.into();
        response.country_code = country_code.parse().unwrap();
        response.filter_applied = format!("Next {} hours from now", hours);

        Ok(Json(ApiResponse::success(response)))
    } else {
        Ok(Json(ApiResponse::error(format!(
            "No data found for next {} hours",
            hours
        ))))
    }
}

/// GET /api/v1/countries
/// List all available countries
async fn list_countries() -> Json<ApiResponse<Vec<String>>> {
    let countries = areas::list_countries()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    Json(ApiResponse::success(countries))
}

#[derive(Serialize)]
struct ZoneInfo {
    code: String,
    name: String,
    tso: Option<String>,
}

/// GET /api/v1/zones/:country
/// Get all bidding zones for a country
async fn get_country_zones(
    Path(country_code): Path<String>,
) -> Result<Json<ApiResponse<Vec<ZoneInfo>>>, StatusCode> {
    if let Some(zones) = areas::get_zones_by_country(&country_code) {
        let zone_info: Vec<_> = zones
            .iter()
            .map(|z| ZoneInfo {
                code: z.code.to_string(),
                name: z.name.to_string(),
                tso: z.tso.map(|s| s.to_string()),
            })
            .collect();

        Ok(Json(ApiResponse::success(zone_info)))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

use askama::Template;
use serde_json::json;

#[derive(Template)]
#[template(path = "plot.html")]
struct PlotTemplate {
    country_code: String,
    country_name: String,
    period_start: String,
    period_end: String,
    data_points: usize,
    plot_data: String,
    plot_layout: String,
}

/// Generate Plotly plot data from surplus series
fn generate_plot_data(surplus_series: &[RenewableSurplus]) -> (String, String) {
    // Extract data
    let timestamps: Vec<String> = surplus_series
        .iter()
        .map(|s| s.timestamp.format("%Y-%m-%d %H:%M").to_string())
        .collect();

    let generation: Vec<f64> = surplus_series.iter().map(|s| s.generation).collect();
    let load: Vec<f64> = surplus_series.iter().map(|s| s.load).collect();
    let surplus: Vec<f64> = surplus_series.iter().map(|s| s.surplus).collect();

    // Create traces
    let traces = json!([
        {
            "x": timestamps,
            "y": generation,
            "name": "Wind + Solar Generation",
            "type": "scatter",
            "mode": "lines+markers",
            "line": {
                "color": "rgb(34, 139, 34)",
                "width": 2
            },
            "marker": {
                "size": 4
            }
        },
        {
            "x": timestamps,
            "y": load,
            "name": "Total Load",
            "type": "scatter",
            "mode": "lines+markers",
            "line": {
                "color": "rgb(30, 144, 255)",
                "width": 2
            },
            "marker": {
                "size": 4
            }
        },
        {
            "x": timestamps,
            "y": surplus,
            "name": "Surplus (Generation - Load)",
            "type": "scatter",
            "mode": "lines+markers",
            "line": {
                "color": "rgb(255, 140, 0)",
                "width": 2
            },
            "marker": {
                "size": 4
            }
        }
    ]);

    // Create layout
    let layout = json!({
        "title": {
            "text": "Renewable Energy Forecast",
            "font": {
                "size": 20
            }
        },
        "xaxis": {
            "title": "Time",
            "tickangle": -45
        },
        "yaxis": {
            "title": "Power (MW)"
        },
        "hovermode": "x unified",
        "plot_bgcolor": "rgb(250, 250, 250)",
        "paper_bgcolor": "white",
        "showlegend": true,
        "legend": {
            "x": 0.01,
            "y": 0.99,
            "bgcolor": "rgba(255, 255, 255, 0.8)",
            "bordercolor": "rgba(0, 0, 0, 0.2)",
            "borderwidth": 1
        }
    });

    (
        serde_json::to_string(&traces).unwrap(),
        serde_json::to_string(&layout).unwrap(),
    )
}

/// GET /api/v1/renewable-surplus/:country/plot
/// Generate interactive Plotly visualization
async fn get_plot(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
    Query(query): Query<TimeQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::BAD_REQUEST)?;

    let hours = query.hours.unwrap_or(24);
    let now = Utc::now();
    let end = now + Duration::hours((hours + 1) as i64);
    let (period_start, period_end) = format_period(now, end);

    let series = state
        .entsoe_client
        .get_renewable_surplus_series(zone.code, &period_start, &period_end)
        .await
        .map_err(|e| {
            tracing::error!("ENTSO-E API error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if series.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let (plot_data, plot_layout) = generate_plot_data(&series);

    let template = PlotTemplate {
        country_code: country_code.clone(),
        country_name: zone.name.to_string(),
        period_start: series
            .first()
            .unwrap()
            .timestamp
            .format("%Y-%m-%d %H:%M UTC")
            .to_string(),
        period_end: series
            .last()
            .unwrap()
            .timestamp
            .format("%Y-%m-%d %H:%M UTC")
            .to_string(),
        data_points: series.len(),
        plot_data,
        plot_layout,
    };

    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(axum::response::Html(html))
}

/// GET /api/v1/renewable-surplus/:country/plot-json
/// Get plot data as JSON (for frontend frameworks)
async fn get_plot_json(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
    Query(query): Query<TimeQuery>,
) -> Result<Json<ApiResponse<PlotData>>, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::BAD_REQUEST)?;

    let hours = query.hours.unwrap_or(24);
    let now = Utc::now();
    let end = now + Duration::hours((hours + 1) as i64);
    let (period_start, period_end) = format_period(now, end);

    let series = state
        .entsoe_client
        .get_renewable_surplus_series(zone.code, &period_start, &period_end)
        .await
        .map_err(|e| {
            tracing::error!("ENTSO-E API error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if series.is_empty() {
        return Ok(Json(ApiResponse::error("No data available".to_string())));
    }

    let plot_data = PlotData {
        timestamps: series.iter().map(|s| s.timestamp.to_rfc3339()).collect(),
        generation: series.iter().map(|s| s.generation).collect(),
        load: series.iter().map(|s| s.load).collect(),
        surplus: series.iter().map(|s| s.surplus).collect(),
    };

    Ok(Json(ApiResponse::success(plot_data)))
}

#[derive(Serialize)]
struct PlotData {
    timestamps: Vec<String>,
    generation: Vec<f64>,
    load: Vec<f64>,
    surplus: Vec<f64>,
}

// ── SEO content pages (server-rendered) ──────────────────────────────────────

struct ForecastRow {
    time: String,
    generation: String,
    load: String,
    surplus: String,
    share: String,
}

/// Build the hourly forecast table rows shared by the country and cloud pages.
fn forecast_rows(series: &[RenewableSurplus]) -> Vec<ForecastRow> {
    series
        .iter()
        .map(|s| ForecastRow {
            time: s.timestamp.format("%a %H:%M").to_string(),
            generation: format!("{:.0}", s.generation),
            load: format!("{:.0}", s.load),
            surplus: format!("{:+.0}", s.surplus),
            share: format!("{:.0}%", s.renewable_share()),
        })
        .collect()
}

/// Pretty provider name for headings and titles.
fn provider_label(provider: &str) -> &'static str {
    match provider {
        "aws" => "AWS",
        "azure" => "Azure",
        "gcp" => "Google Cloud",
        _ => "Cloud",
    }
}

#[derive(Template)]
#[template(path = "country.html")]
struct CountryPageTemplate {
    title: String,
    meta_description: String,
    canonical_url: String,
    json_ld: String,
    country_name: String,
    updated_utc: String,
    sentences: Vec<String>,
    rows: Vec<ForecastRow>,
    plot_data: String,
    plot_layout: String,
}

/// GET /electricity/{country}
/// Server-rendered, crawlable forecast page: an inline interactive chart,
/// auto-generated descriptive text, an hourly data table, and meta/JSON-LD.
async fn get_country_page(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::NOT_FOUND)?;
    let code_lower = country_code.to_lowercase();
    let country_name = zone.name.to_string();

    let now = Utc::now();
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    let summary = summarize_forecast(&series, &country_name);
    let rows = forecast_rows(&series);
    let (plot_data, plot_layout) = generate_plot_data(&series);

    let title = format!(
        "When is electricity greenest in {country_name}? Renewable energy forecast | educk"
    );
    let meta_description = summary.meta_description(&country_name);
    let canonical_url = format!("{}/electricity/{}", state.base_url, code_lower);
    let updated_utc = now.format("%Y-%m-%d %H:%M UTC").to_string();

    let json_ld = json!({
        "@context": "https://schema.org",
        "@type": "WebPage",
        "name": title,
        "description": meta_description,
        "url": canonical_url,
        "inLanguage": "en",
        "dateModified": now.to_rfc3339(),
        "isPartOf": {
            "@type": "WebSite",
            "name": "educk",
            "url": state.base_url.to_string(),
        },
        "about": {
            "@type": "Thing",
            "name": format!("Renewable electricity in {country_name}"),
        },
    })
    .to_string();

    let template = CountryPageTemplate {
        title,
        meta_description,
        canonical_url,
        json_ld,
        country_name,
        updated_utc,
        sentences: summary.sentences,
        rows,
        plot_data,
        plot_layout,
    };

    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(axum::response::Html(html))
}

struct CountryLink {
    code: String,
    name: String,
    url: String,
    selected: bool,
}

struct CloudLink {
    region: String,
    location: String,
    url: String,
}

struct CloudProviderGroup {
    provider_label: String,
    regions: Vec<CloudLink>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct LandingTemplate {
    title: String,
    meta_description: String,
    canonical_url: String,
    json_ld: String,
    selected_name: String,
    updated_utc: String,
    sentences: Vec<String>,
    rows: Vec<ForecastRow>,
    plot_data: String,
    plot_layout: String,
    countries: Vec<CountryLink>,
    cloud_providers: Vec<CloudProviderGroup>,
}

#[derive(Deserialize)]
struct LandingQuery {
    country: Option<String>,
}

/// GET /?country=XX
/// The web frontend: an interactive chart + descriptive text + hourly table for
/// the selected country (default DE), with a country picker and the full
/// country/cloud index below. Forecast data is served from a short-lived cache.
async fn get_landing(
    State(state): State<AppState>,
    Query(query): Query<LandingQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    // Resolve the selected country, falling back to the default if unknown.
    let selected = query
        .country
        .as_deref()
        .and_then(get_primary_zone)
        .or_else(|| get_primary_zone("DE"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let selected_code = selected.country_code;
    let selected_name = selected.name.to_string();

    let mut countries: Vec<CountryLink> = areas::list_countries()
        .into_iter()
        .filter_map(|code| {
            get_primary_zone(code).map(|zone| CountryLink {
                code: code.to_string(),
                name: zone.name.to_string(),
                url: format!("/electricity/{}", code.to_lowercase()),
                selected: code == selected_code,
            })
        })
        .collect();
    countries.sort_by(|a, b| a.name.cmp(&b.name));

    // Forecast for the selected country (cached; landing degrades gracefully).
    let now = Utc::now();
    let series = state.series_for_zone(selected.code).await.unwrap_or_default();
    let summary = summarize_forecast(&series, &selected_name);
    let rows = forecast_rows(&series);
    let (plot_data, plot_layout) = generate_plot_data(&series);
    let updated_utc = now.format("%Y-%m-%d %H:%M UTC").to_string();

    // Cloud regions grouped by provider. `all_regions()` is sorted by
    // (provider, region), so consecutive grouping preserves that order.
    let mut cloud_providers: Vec<CloudProviderGroup> = Vec::new();
    for cr in cloud::all_regions() {
        let label = provider_label(cr.provider).to_string();
        let link = CloudLink {
            region: cr.region.to_string(),
            location: cr.location.to_string(),
            url: format!("/cloud/{}/{}", cr.provider, cr.region),
        };
        match cloud_providers.last_mut() {
            Some(group) if group.provider_label == label => group.regions.push(link),
            _ => cloud_providers.push(CloudProviderGroup {
                provider_label: label,
                regions: vec![link],
            }),
        }
    }

    let title = "educk — when is electricity greenest across Europe?".to_string();
    let meta_description = "educk shows live and day-ahead renewable electricity share across \
         European grids, so you can shift energy use to cleaner, lower-carbon hours."
        .to_string();
    let canonical_url = format!("{}/", state.base_url);

    let list_items: Vec<_> = countries
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "@type": "ListItem",
                "position": i + 1,
                "name": format!("Renewable electricity in {}", c.name),
                "url": format!("{}{}", state.base_url, c.url),
            })
        })
        .collect();

    let json_ld = json!({
        "@context": "https://schema.org",
        "@graph": [
            {
                "@type": "WebSite",
                "name": "educk",
                "url": state.base_url.to_string(),
                "description": meta_description,
                "inLanguage": "en",
            },
            {
                "@type": "ItemList",
                "name": "Renewable electricity forecast by country",
                "itemListElement": list_items,
            }
        ],
    })
    .to_string();

    let template = LandingTemplate {
        title,
        meta_description,
        canonical_url,
        json_ld,
        selected_name,
        updated_utc,
        sentences: summary.sentences,
        rows,
        plot_data,
        plot_layout,
        countries,
        cloud_providers,
    };

    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(axum::response::Html(html))
}

#[derive(Template)]
#[template(path = "cloud.html")]
struct CloudPageTemplate {
    title: String,
    meta_description: String,
    canonical_url: String,
    json_ld: String,
    provider_label: String,
    region: String,
    location: String,
    country_name: String,
    updated_utc: String,
    sentences: Vec<String>,
    rows: Vec<ForecastRow>,
    plot_data: String,
    plot_layout: String,
    country_url: String,
}

/// GET /cloud/{provider}/{region}
/// Server-rendered, crawlable forecast page for a cloud region, framed for
/// carbon-aware workload scheduling. Uses the region's underlying national grid.
async fn get_cloud_page(
    State(state): State<AppState>,
    Path((provider, region)): Path<(String, String)>,
) -> Result<impl IntoResponse, StatusCode> {
    let cr = cloud::lookup(&provider, &region).ok_or(StatusCode::NOT_FOUND)?;
    let zone = get_primary_zone(cr.country_code).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let country_name = zone.name.to_string();

    let now = Utc::now();
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    let summary = summarize_forecast(&series, &country_name);
    let rows = forecast_rows(&series);
    let (plot_data, plot_layout) = generate_plot_data(&series);

    let label = provider_label(cr.provider);
    let title = format!(
        "Greenest time to run workloads in {}/{} ({}) | educk",
        cr.provider, cr.region, cr.location
    );
    let meta_description = summary.meta_description(&country_name);
    let canonical_url = format!("{}/cloud/{}/{}", state.base_url, cr.provider, cr.region);
    let updated_utc = now.format("%Y-%m-%d %H:%M UTC").to_string();

    let json_ld = json!({
        "@context": "https://schema.org",
        "@type": "WebPage",
        "name": title,
        "description": meta_description,
        "url": canonical_url,
        "inLanguage": "en",
        "dateModified": now.to_rfc3339(),
        "isPartOf": {
            "@type": "WebSite",
            "name": "educk",
            "url": state.base_url.to_string(),
        },
        "about": {
            "@type": "Thing",
            "name": format!("Carbon-aware scheduling for {} {}", label, cr.region),
        },
    })
    .to_string();

    let template = CloudPageTemplate {
        title,
        meta_description,
        canonical_url,
        json_ld,
        provider_label: label.to_string(),
        region: cr.region.to_string(),
        location: cr.location.to_string(),
        country_name,
        updated_utc,
        sentences: summary.sentences,
        rows,
        plot_data,
        plot_layout,
        country_url: format!("/electricity/{}", cr.country_code.to_lowercase()),
    };

    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(axum::response::Html(html))
}

// ── Legal pages (Impressum / privacy policy) ─────────────────────────────────

#[derive(Template)]
#[template(path = "impressum.html")]
struct ImpressumTemplate {
    canonical_url: String,
}

/// GET /impressum
/// Static legal notice (Impressum) required of a German operator under § 5 DDG.
async fn get_impressum(State(state): State<AppState>) -> Result<impl IntoResponse, StatusCode> {
    let template = ImpressumTemplate {
        canonical_url: format!("{}/impressum", state.base_url),
    };
    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(axum::response::Html(html))
}

#[derive(Template)]
#[template(path = "privacy.html")]
struct PrivacyTemplate {
    canonical_url: String,
}

/// GET /privacy
/// Static GDPR privacy policy (Datenschutzerklärung); discloses Google Analytics.
async fn get_privacy(State(state): State<AppState>) -> Result<impl IntoResponse, StatusCode> {
    let template = PrivacyTemplate {
        canonical_url: format!("{}/privacy", state.base_url),
    };
    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(axum::response::Html(html))
}

// ── Cloud provider endpoints ─────────────────────────────────────────────────

#[derive(Serialize)]
struct CloudRegionInfo {
    provider: String,
    region: String,
    location: String,
    country_code: String,
}

#[derive(Serialize)]
struct CloudBestWindowResponse {
    cloud_region: CloudRegionInfo,
    timestamp: String,
    timestamp_utc: String,
    generation_mw: f64,
    load_mw: f64,
    surplus_mw: f64,
    surplus_percentage: f64,
    hours_searched: u32,
}

/// GET /api/v1/cloud/{provider}/{region}/next?hours=N
/// Best renewable surplus window for a cloud provider region.
async fn get_cloud_best_window(
    State(state): State<AppState>,
    Path((provider, region)): Path<(String, String)>,
    Query(query): Query<TimeQuery>,
) -> Result<Json<ApiResponse<CloudBestWindowResponse>>, StatusCode> {
    let cr = cloud::lookup(&provider, &region).ok_or_else(|| {
        tracing::warn!("unknown cloud region: {provider}/{region}");
        StatusCode::NOT_FOUND
    })?;

    let zone = get_primary_zone(cr.country_code).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let hours = query.hours.unwrap_or(24);
    let now = Utc::now();
    let end = now + Duration::hours((hours + 1) as i64);
    let (period_start, period_end) = format_period(now, end);

    let series = state
        .entsoe_client
        .get_renewable_surplus_series(zone.code, &period_start, &period_end)
        .await
        .map_err(|e| {
            tracing::error!("ENTSO-E API error for {}/{}: {}", provider, region, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let best = find_max(series).ok_or_else(|| {
        tracing::warn!("no surplus data for {provider}/{region}");
        StatusCode::NOT_FOUND
    })?;

    Ok(Json(ApiResponse::success(CloudBestWindowResponse {
        cloud_region: CloudRegionInfo {
            provider: cr.provider.to_string(),
            region: cr.region.to_string(),
            location: cr.location.to_string(),
            country_code: cr.country_code.to_string(),
        },
        timestamp: best.timestamp.to_rfc3339(),
        timestamp_utc: best.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        generation_mw: best.generation,
        load_mw: best.load,
        surplus_mw: best.surplus,
        surplus_percentage: best.surplus_percentage(),
        hours_searched: hours,
    })))
}

/// GET /api/v1/cloud/regions
/// List all supported cloud provider regions.
async fn list_cloud_regions() -> Json<ApiResponse<Vec<CloudRegionInfo>>> {
    let regions = cloud::all_regions()
        .into_iter()
        .map(|cr| CloudRegionInfo {
            provider: cr.provider.to_string(),
            region: cr.region.to_string(),
            location: cr.location.to_string(),
            country_code: cr.country_code.to_string(),
        })
        .collect();
    Json(ApiResponse::success(regions))
}

/// GET /health
async fn health() -> &'static str {
    "OK"
}

/// GET /robots.txt
/// Allow all crawlers and advertise the sitemap.
async fn robots_txt(State(state): State<AppState>) -> impl IntoResponse {
    let body = format!(
        "User-agent: *\nAllow: /\n\nSitemap: {}/sitemap.xml\n",
        state.base_url
    );
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body,
    )
}

/// GET /sitemap.xml
/// Lists the landing page and every per-country and per-cloud-region content page.
async fn sitemap_xml(State(state): State<AppState>) -> impl IntoResponse {
    let mut urls = format!("  <url><loc>{}/</loc></url>\n", state.base_url);
    for code in areas::list_countries() {
        urls.push_str(&format!(
            "  <url><loc>{}/electricity/{}</loc></url>\n",
            state.base_url,
            code.to_lowercase()
        ));
    }
    for cr in cloud::all_regions() {
        urls.push_str(&format!(
            "  <url><loc>{}/cloud/{}/{}</loc></url>\n",
            state.base_url, cr.provider, cr.region
        ));
    }
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
         {urls}</urlset>\n"
    );
    (
        [(header::CONTENT_TYPE, "application/xml; charset=utf-8")],
        body,
    )
}

pub async fn start_server(config: Config) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "educk_rs=debug,tower_http=debug".parse().unwrap()),
        )
        .init();

    let state = AppState {
        entsoe_client: Arc::new(EntsoeClient::new(config.entsoe_api_key)),
        base_url: Arc::from(config.public_base_url.trim_end_matches('/')),
        series_cache: Arc::new(Mutex::new(HashMap::new())),
    };

    // Proactively warm the DE cache so the landing page is always fast.
    let warmer = state.clone();
    tokio::spawn(async move {
        if let Some(zone) = get_primary_zone("DE") {
            loop {
                if let Err(e) = warmer.series_for_zone(zone.code).await {
                    tracing::warn!("DE cache warm-up failed: {:?}", e);
                } else {
                    tracing::debug!("DE cache refreshed");
                }
                tokio::time::sleep(CACHE_TTL).await;
            }
        }
    });

    let app = Router::new()
        .route("/", get(get_landing))
        .route("/health", get(health))
        .route("/robots.txt", get(robots_txt))
        .route("/sitemap.xml", get(sitemap_xml))
        .route("/electricity/{country}", get(get_country_page))
        .route("/cloud/{provider}/{region}", get(get_cloud_page))
        .route("/impressum", get(get_impressum))
        .route("/privacy", get(get_privacy))
        .route("/api/v1/countries", get(list_countries))
        .route("/api/v1/zones/{country}", get(get_country_zones))
        .route(
            "/api/v1/renewable-surplus/{country}/night",
            get(get_night_surplus),
        )
        .route(
            "/api/v1/renewable-surplus/{country}/next-6h",
            get(get_next_6h_surplus),
        )
        .route(
            "/api/v1/renewable-surplus/{country}/next-24h",
            get(get_next_24h_surplus),
        )
        .route(
            "/api/v1/renewable-surplus/{country}/next",
            get(get_custom_hours_surplus),
        )
        .route("/api/v1/renewable-surplus/{country}/plot", get(get_plot))
        .route(
            "/api/v1/renewable-surplus/{country}/plot-json",
            get(get_plot_json),
        )
        .route("/api/v1/cloud/regions", get(list_cloud_regions))
        .route(
            "/api/v1/cloud/{provider}/{region}/next",
            get(get_cloud_best_window),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3044").await?;
    tracing::info!("server listening on http://0.0.0.0:3044");

    axum::serve(listener, app).await?;

    Ok(())
}
