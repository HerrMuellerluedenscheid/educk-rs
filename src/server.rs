use axum::{
    Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Json},
    routing::get,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::cloud;
use crate::config::Config;
use crate::entsoe::analysis::{RenewableSurplus, summarize_forecast};
use crate::entsoe::areas::get_primary_zone;
use crate::entsoe::{EntsoeClient, areas};

#[derive(Clone)]
struct AppState {
    entsoe_client: Arc<EntsoeClient>,
    /// Public origin for canonical URLs / Open Graph tags on SSR pages.
    base_url: Arc<str>,
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
    app_url: String,
    chart_url: String,
}

/// GET /electricity/{country}
/// Server-rendered, crawlable forecast page with auto-generated descriptive text,
/// an hourly data table, canonical/Open Graph tags and JSON-LD. Links to the
/// interactive Flutter dashboard at /app.
async fn get_country_page(
    State(state): State<AppState>,
    Path(country_code): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::NOT_FOUND)?;
    let code_lower = country_code.to_lowercase();
    let country_name = zone.name.to_string();

    let now = Utc::now();
    let end = now + Duration::hours(25);
    let (period_start, period_end) = format_period(now, end);

    let series = state
        .entsoe_client
        .get_renewable_surplus_series(zone.code, &period_start, &period_end)
        .await
        .map_err(|e| {
            tracing::error!("ENTSO-E API error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let summary = summarize_forecast(&series, &country_name);

    let rows: Vec<ForecastRow> = series
        .iter()
        .map(|s| ForecastRow {
            time: s.timestamp.format("%a %H:%M").to_string(),
            generation: format!("{:.0}", s.generation),
            load: format!("{:.0}", s.load),
            surplus: format!("{:+.0}", s.surplus),
            share: format!("{:.0}%", s.renewable_share()),
        })
        .collect();

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
        // TODO: /app still needs deploy wiring (Flutter served behind apex).
        app_url: format!("{}/app?country={}", state.base_url, code_lower),
        chart_url: format!("/api/v1/renewable-surplus/{}/plot", code_lower),
    };

    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(axum::response::Html(html))
}

struct CountryLink {
    name: String,
    url: String,
}

#[derive(Template)]
#[template(path = "index.html")]
struct LandingTemplate {
    title: String,
    meta_description: String,
    canonical_url: String,
    json_ld: String,
    app_url: String,
    countries: Vec<CountryLink>,
}

/// GET /
/// Server-rendered landing page: explains educk and links to every per-country
/// content page (crawl discovery) and the interactive dashboard. Intentionally
/// makes no upstream API calls — this is the most-hit/most-crawled route and has
/// no caching layer yet.
async fn get_landing(State(state): State<AppState>) -> Result<impl IntoResponse, StatusCode> {
    let mut countries: Vec<CountryLink> = areas::list_countries()
        .into_iter()
        .filter_map(|code| {
            get_primary_zone(code).map(|zone| CountryLink {
                name: zone.name.to_string(),
                url: format!("/electricity/{}", code.to_lowercase()),
            })
        })
        .collect();
    countries.sort_by(|a, b| a.name.cmp(&b.name));

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
        app_url: format!("{}/app", state.base_url),
        countries,
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
/// Lists the landing page and every per-country content page.
async fn sitemap_xml(State(state): State<AppState>) -> impl IntoResponse {
    let mut urls = format!("  <url><loc>{}/</loc></url>\n", state.base_url);
    for code in areas::list_countries() {
        urls.push_str(&format!(
            "  <url><loc>{}/electricity/{}</loc></url>\n",
            state.base_url,
            code.to_lowercase()
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
    };

    let app = Router::new()
        .route("/", get(get_landing))
        .route("/health", get(health))
        .route("/robots.txt", get(robots_txt))
        .route("/sitemap.xml", get(sitemap_xml))
        .route("/electricity/{country}", get(get_country_page))
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
