use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::CorsLayer;

use crate::entsoe::analysis::RenewableSurplus;
use crate::entsoe::areas::get_primary_zone;
use crate::entsoe::{EntsoeClient, areas};

#[derive(Clone)]
struct AppState {
    entsoe_client: Arc<EntsoeClient>,
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
            eprintln!("ENTSO-E API error: {}", e);
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
            eprintln!("ENTSO-E API error: {}", e);
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
            eprintln!("ENTSO-E API error: {}", e);
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
        eprintln!("Template rendering error: {}", e);
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
            eprintln!("ENTSO-E API error: {}", e);
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

/// GET /health
async fn health() -> &'static str {
    "OK"
}

pub async fn start_server() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let api_key =
        std::env::var("ENTSOE_API_KEY").expect("ENTSOE_API_KEY environment variable not set");

    let state = AppState {
        entsoe_client: Arc::new(EntsoeClient::new(api_key)),
    };

    let app = Router::new()
        .route("/health", get(health))
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
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3044").await?;
    println!("🚀 Server running on http://0.0.0.0:3044");
    println!("\nAvailable endpoints:");
    println!("  GET /health");
    println!("  GET /api/v1/countries");
    println!("  GET /api/v1/zones/:country");
    println!("  GET /api/v1/renewable-surplus/:country/night");
    println!("  GET /api/v1/renewable-surplus/:country/next-6h");
    println!("  GET /api/v1/renewable-surplus/:country/next-24h");
    println!("  GET /api/v1/renewable-surplus/:country/next?hours=N");
    println!("  GET /api/v1/renewable-surplus/:country/plot?hours=N");
    println!("  GET /api/v1/renewable-surplus/:country/plot-json?hours=N");
    println!("\nExamples:");
    println!("  curl http://localhost:3044/api/v1/renewable-surplus/DE/night");

    axum::serve(listener, app).await?;

    Ok(())
}
