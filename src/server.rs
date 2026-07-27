use axum::{
    Router,
    extract::{OriginalUri, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Json, Response},
    routing::get,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::cloud;
use crate::config::Config;
use crate::entsoe::analysis::{RenewableSurplus, summarize_forecast};
use crate::i18n::{self, Lang};
use crate::entsoe::areas::get_primary_zone;
use crate::entsoe::{EntsoeClient, areas};
use crate::proxy::{self, ModelledPoint};
use crate::weather::WeatherClient;

/// How far ahead the SSR pages forecast, and how long a fetched series is reused.
const LOOKAHEAD_HOURS: i64 = 25;
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(3600);
/// While upstream is failing, how long to keep serving the last-known-good series
/// before re-attempting a fetch. Stops every request from waiting on (and
/// hammering) a downed ENTSO-E for the full `CACHE_TTL`.
const STALE_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(120);

/// A cached forecast series plus the bookkeeping needed to serve it stale during
/// an upstream outage without re-fetching on every request.
#[derive(Clone)]
struct CacheEntry {
    /// When the data was last successfully fetched (drives `CACHE_TTL`).
    fetched_at: Instant,
    /// When a fetch was last attempted (drives `STALE_RETRY_INTERVAL` backoff).
    last_attempt: Instant,
    data: Vec<RenewableSurplus>,
}

type SeriesCache = Arc<Mutex<HashMap<String, CacheEntry>>>;
/// Cache for the modelled (MaStR × weather) German series; a single entry since
/// the overlay is DE-only.
type ModelledCache = Arc<Mutex<Option<(Instant, Vec<ModelledPoint>)>>>;

#[derive(Clone)]
struct AppState {
    entsoe_client: Arc<EntsoeClient>,
    /// Open-Meteo client for the location-based renewable proxy.
    weather_client: Arc<WeatherClient>,
    /// Public origin for canonical URLs / Open Graph tags on SSR pages.
    base_url: Arc<str>,
    /// Per-zone forecast cache so the SSR pages (esp. the apex landing) don't
    /// hit ENTSO-E on every request. Day-ahead data changes slowly.
    series_cache: SeriesCache,
    /// Cached modelled German wind+solar series (weather refreshes hourly).
    modelled_cache: ModelledCache,
}

impl AppState {
    /// Fetch the next ~25h renewable surplus series for a bidding zone, reusing a
    /// cached result younger than `CACHE_TTL`.
    async fn series_for_zone(&self, zone_code: &str) -> Result<Vec<RenewableSurplus>, StatusCode> {
        {
            let cache = self.series_cache.lock().unwrap();
            if let Some(entry) = cache.get(zone_code) {
                // Fresh data within the TTL.
                if entry.fetched_at.elapsed() < CACHE_TTL {
                    return Ok(entry.data.clone());
                }
                // Stale data, but we tried to refresh very recently — upstream is
                // likely still down, so serve stale instead of waiting on it again.
                if entry.last_attempt.elapsed() < STALE_RETRY_INTERVAL {
                    return Ok(entry.data.clone());
                }
            }
        } // drop the guard before awaiting

        let now = Utc::now();
        let end = now + Duration::hours(LOOKAHEAD_HOURS);
        let (period_start, period_end) = format_period(now, end);

        match self
            .entsoe_client
            .get_renewable_surplus_series(zone_code, &period_start, &period_end)
            .await
        {
            Ok(series) => {
                let now = Instant::now();
                self.series_cache.lock().unwrap().insert(
                    zone_code.to_string(),
                    CacheEntry {
                        fetched_at: now,
                        last_attempt: now,
                        data: series.clone(),
                    },
                );
                Ok(series)
            }
            // Upstream failed (e.g. ENTSO-E maintenance). Serve the last-known-good
            // series if we have one — the dashboard and SSR pages then show
            // slightly stale but useful data rather than an error.
            Err(e) => {
                let mut cache = self.series_cache.lock().unwrap();
                if let Some(entry) = cache.get_mut(zone_code) {
                    entry.last_attempt = Instant::now();
                    tracing::warn!(
                        "ENTSO-E fetch for {zone_code} failed ({e}); serving cached series \
                         from {:.0?} ago",
                        entry.fetched_at.elapsed()
                    );
                    return Ok(entry.data.clone());
                }
                tracing::error!("ENTSO-E API error and no cached series for {zone_code}: {e}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    }

    /// Cached series for a zone *without* triggering an upstream fetch. Used by
    /// the landing page to server-render a first-paint dashboard only when data
    /// is already warm — so `/` never blocks on ENTSO-E (cold cache → the client
    /// dashboard fetches on its own). Returns the last-known-good series, even if
    /// past its TTL (a day-ahead forecast stays useful well beyond 10 minutes).
    fn cached_series(&self, zone_code: &str) -> Option<Vec<RenewableSurplus>> {
        let cache = self.series_cache.lock().unwrap();
        cache
            .get(zone_code)
            .filter(|e| !e.data.is_empty())
            .map(|e| e.data.clone())
    }

    /// The modelled (MaStR × weather) German wind+solar series, calibrated to the
    /// supplied ENTSO-E series. Cached for `CACHE_TTL`. Returns `None` when the
    /// capacity grid is empty (no ingest yet) or the weather fetch fails — the
    /// page then simply renders without the overlay.
    async fn modelled_de(&self, entsoe: &[RenewableSurplus]) -> Option<Vec<ModelledPoint>> {
        {
            let cache = self.modelled_cache.lock().unwrap();
            if let Some((fetched_at, data)) = cache.as_ref() {
                if fetched_at.elapsed() < CACHE_TTL {
                    return Some(data.clone());
                }
            }
        } // drop the guard before awaiting

        match proxy::modelled_de_series(&self.weather_client, entsoe).await {
            Ok(Some(series)) => {
                *self.modelled_cache.lock().unwrap() = Some((Instant::now(), series.clone()));
                Some(series)
            }
            Ok(None) => None, // empty capacity grid
            Err(e) => {
                tracing::warn!("modelled DE series failed: {e:?}");
                None
            }
        }
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

    // Cached, stale-on-error; degrades to an empty series when upstream is down.
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();
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

    // Cached, stale-on-error; degrades to an empty series when upstream is down.
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();
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

/// Generate Plotly plot data from a surplus series, optionally overlaying the
/// modelled (MaStR × weather) wind+solar series as an extra trace.
fn generate_plot_data(
    surplus_series: &[RenewableSurplus],
    modelled: Option<&[ModelledPoint]>,
) -> (String, String) {
    // Extract data
    let timestamps: Vec<String> = surplus_series
        .iter()
        .map(|s| s.timestamp.format("%Y-%m-%d %H:%M").to_string())
        .collect();

    let generation: Vec<f64> = surplus_series.iter().map(|s| s.generation).collect();
    let load: Vec<f64> = surplus_series.iter().map(|s| s.load).collect();
    let surplus: Vec<f64> = surplus_series.iter().map(|s| s.surplus).collect();

    // Create traces
    let mut traces = vec![
        json!({
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
        }),
        json!({
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
        }),
        json!({
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
        }),
    ];

    // Optional cross-check: bottom-up estimate from plant locations × weather.
    if let Some(modelled) = modelled {
        let m_timestamps: Vec<String> = modelled
            .iter()
            .map(|m| m.timestamp.format("%Y-%m-%d %H:%M").to_string())
            .collect();
        let m_total: Vec<f64> = modelled.iter().map(|m| m.total_mw).collect();
        traces.push(json!({
            "x": m_timestamps,
            "y": m_total,
            "name": "Modelled wind + solar (MaStR × weather)",
            "type": "scatter",
            "mode": "lines",
            "line": {
                "color": "rgb(148, 0, 211)",
                "width": 2,
                "dash": "dot"
            }
        }));
    }

    let traces = json!(traces);

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
) -> Result<impl IntoResponse, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::BAD_REQUEST)?;

    // Cached, stale-on-error; the plot still 404s if we have nothing to draw.
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    if series.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let (plot_data, plot_layout) = generate_plot_data(&series, None);

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
) -> Result<Json<ApiResponse<PlotData>>, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::BAD_REQUEST)?;

    // Cached, stale-on-error. When upstream is down and we have no cached series,
    // this degrades to a clean `success: false` body (HTTP 200) the dashboard
    // surfaces as an error — instead of a 500 that reads as a request timeout.
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    if series.is_empty() {
        return Ok(Json(ApiResponse::error(
            "Forecast data is temporarily unavailable. Please try again shortly.".to_string(),
        )));
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

// ── Language negotiation (?lang=de + soft redirect) ──────────────────────────
// English lives at the bare paths; German is the same path with `?lang=de`. We
// never render German at a bare URL by header (that would hide it from crawlers);
// instead a German-preferring human on a bare URL is 302'd to the `?lang=de`
// variant, and the choice is remembered in an `educk_lang` cookie.

/// Name of the cookie that remembers an explicit language choice.
const LANG_COOKIE: &str = "educk_lang";

/// The outcome of negotiating a language for a bare (no `?lang`) or explicit request.
enum LangChoice {
    /// Render in `lang`; when `persist`, set the `educk_lang` cookie.
    Render { lang: Lang, persist: bool },
    /// 302 the visitor to the German `?lang=de` variant of the current URL.
    RedirectDe,
}

/// Read a cookie value out of the `Cookie` request header.
fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|kv| {
        let kv = kv.trim();
        kv.strip_prefix(name).and_then(|rest| rest.strip_prefix('='))
    })
}

/// Decide the page language. Precedence: explicit `?lang` → `educk_lang` cookie →
/// `Accept-Language`. Crawlers (Accept-Language: en / none) always resolve to
/// English, so the bare URLs stay indexable.
fn negotiate_lang(headers: &HeaderMap, query_lang: Option<&str>) -> LangChoice {
    if let Some(lang) = Lang::from_query(query_lang) {
        // An explicit choice (from the switcher or a shared link) is remembered.
        return LangChoice::Render { lang, persist: true };
    }
    match cookie_value(headers, LANG_COOKIE) {
        Some("de") => return LangChoice::RedirectDe,
        Some("en") => return LangChoice::Render { lang: Lang::En, persist: false },
        _ => {}
    }
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match i18n::preferred_from_accept_language(accept) {
        Lang::De => LangChoice::RedirectDe,
        Lang::En => LangChoice::Render {
            lang: Lang::En,
            persist: false,
        },
    }
}

/// Resolve the language for an SSR handler, short-circuiting to a 302 when the
/// visitor should be sent to the German variant. Returns `(lang, persist)`.
fn resolve_lang(
    headers: &HeaderMap,
    uri: &OriginalUri,
    query_lang: Option<&str>,
) -> Result<(Lang, bool), Response> {
    match negotiate_lang(headers, query_lang) {
        LangChoice::Render { lang, persist } => Ok((lang, persist)),
        LangChoice::RedirectDe => {
            let path_q = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
            let target = i18n::add_lang_param(path_q, "de");
            // 302 Found: a *temporary* content-negotiation redirect, so the bare
            // English URL stays the indexable canonical (a 301/308 would hand the
            // ranking signals to the German variant).
            Err((StatusCode::FOUND, [(header::LOCATION, target)]).into_response())
        }
    }
}

/// Attach the `educk_lang` cookie to a response when an explicit choice was made.
fn with_lang_cookie(mut resp: Response, lang: Lang, persist: bool) -> Response {
    if persist {
        let v = format!(
            "{LANG_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax",
            lang.code()
        );
        if let Ok(hv) = HeaderValue::from_str(&v) {
            resp.headers_mut().append(header::SET_COOKIE, hv);
        }
    }
    resp
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

/// Query params for the bilingual SSR pages (the `?lang=` selector).
#[derive(Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

#[derive(Template)]
#[template(path = "country.html")]
struct CountryPageTemplate {
    t: &'static i18n::Strings,
    title: String,
    meta_description: String,
    canonical_url: String,
    alt_en: String,
    alt_de: String,
    json_ld: String,
    home_url: String,
    impressum_url: String,
    privacy_url: String,
    support_url: String,
    h1: String,
    lead: String,
    hourly_h2: String,
    table_caption: String,
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
    headers: HeaderMap,
    uri: OriginalUri,
    Path(country_code): Path<String>,
    Query(q): Query<LangQuery>,
) -> Result<Response, StatusCode> {
    let zone = get_primary_zone(&country_code).ok_or(StatusCode::NOT_FOUND)?;
    let (lang, persist) = match resolve_lang(&headers, &uri, q.lang.as_deref()) {
        Ok(v) => v,
        Err(redirect) => return Ok(redirect),
    };
    let t = i18n::strings(lang);
    let code_lower = country_code.to_lowercase();
    let country_name = i18n::country_name(zone.name, lang);

    let now = Utc::now();
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    // Germany gets the bottom-up MaStR × weather cross-check overlaid on the chart.
    let modelled = if zone.country_code == "DE" {
        state.modelled_de(&series).await
    } else {
        None
    };

    let summary = summarize_forecast(&series, &country_name, lang);
    let rows = forecast_rows(&series);
    let (plot_data, plot_layout) = generate_plot_data(&series, modelled.as_deref());

    let path = format!("/electricity/{code_lower}");
    let (canonical_url, alt_en, alt_de) = i18n::page_urls(&state.base_url, &path, lang);
    let title = i18n::country_title(lang, &country_name);
    let meta_description = summary.meta_description(&country_name, lang);
    let updated_utc = now.format("%Y-%m-%d %H:%M UTC").to_string();

    let json_ld = json!({
        "@context": "https://schema.org",
        "@type": "WebPage",
        "name": title,
        "description": meta_description,
        "url": canonical_url,
        "inLanguage": lang.code(),
        "dateModified": now.to_rfc3339(),
        "isPartOf": {
            "@type": "WebSite",
            "name": "educk",
            "url": state.base_url.to_string(),
        },
        "about": {
            "@type": "Thing",
            "name": i18n::country_about(lang, &country_name),
        },
    })
    .to_string();

    let template = CountryPageTemplate {
        t,
        title,
        meta_description,
        canonical_url,
        alt_en,
        alt_de,
        json_ld,
        home_url: i18n::localize_url("/", lang),
        impressum_url: i18n::localize_url("/impressum", lang),
        privacy_url: i18n::localize_url("/privacy", lang),
        support_url: i18n::localize_url("/support", lang),
        h1: i18n::country_h1(lang, &country_name),
        lead: i18n::country_lead(lang, &country_name),
        hourly_h2: i18n::country_hourly_h2(lang, &country_name),
        table_caption: i18n::caption_country(lang).to_string(),
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

    Ok(with_lang_cookie(Html(html).into_response(), lang, persist))
}

struct CountryLink {
    name: String,
    url: String,
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

/// Server-rendered first-paint snapshot of the dashboard. The same markup is
/// then hydrated (and made interactive + live) by `static/app/app.js`, which
/// reads the embedded `data_json` instead of re-fetching. Mirrors the gauge /
/// cards / overview-chart / hint logic in app.js so first paint matches.
struct DashboardView {
    data_json: String, // {country, timestamps, generation, load, surplus} for app.js
    // gauge (full inner SVG of #gauge)
    gauge_html: String,
    // current-status card
    current_title: &'static str,
    current_value: String,
    current_sub: String,
    current_trend: &'static str,
    current_card_style: String,
    current_value_style: String,
    // peak card
    peak_time: String,
    peak_value: String,
    // coverage card
    coverage_value: String,
    coverage_value_style: String,
    coverage_card_style: String,
    // chart + range + hint
    chart_svg: String,
    range_text: String,
    hint_text: String,
    hint_style: String,
}

// Gauge geometry — matches static/app/app.js (clockwise sweep from `START`).
const GAUGE_START: f64 = 150.0;
const GAUGE_SWEEP: f64 = 240.0;

fn gauge_polar(cx: f64, cy: f64, r: f64, deg: f64) -> (f64, f64) {
    let a = deg.to_radians();
    (cx + r * a.cos(), cy + r * a.sin())
}

fn gauge_arc_path(cx: f64, cy: f64, r: f64, start: f64, sweep: f64) -> String {
    let (x0, y0) = gauge_polar(cx, cy, r, start);
    let (x1, y1) = gauge_polar(cx, cy, r, start + sweep);
    let large = if sweep > 180.0 { 1 } else { 0 };
    format!("M {x0:.2} {y0:.2} A {r} {r} 0 {large} 1 {x1:.2} {y1:.2}")
}

/// red → amber → green, interpolated by `t` in [0, 1].
fn gauge_color(t: f64) -> String {
    let stops = [[0xE5, 0x39, 0x35], [0xFB, 0x8C, 0x00], [0x2E, 0x7D, 0x32]];
    let (a, b, k) = if t < 0.5 {
        (stops[0], stops[1], t / 0.5)
    } else {
        (stops[1], stops[2], (t - 0.5) / 0.5)
    };
    let c: Vec<i64> = (0..3)
        .map(|i| (a[i] as f64 + (b[i] as f64 - a[i] as f64) * k).round() as i64)
        .collect();
    format!("rgb({},{},{})", c[0], c[1], c[2])
}

/// Human-readable MW/GW, signed. Mirrors `fmtMW` in app.js.
fn fmt_mw(v: f64) -> String {
    let (a, s) = (v.abs(), if v < 0.0 { "-" } else { "" });
    if a >= 1000.0 {
        format!("{s}{:.1} GW", a / 1000.0)
    } else {
        format!("{s}{:.0} MW", a)
    }
}

fn fmt_hm(ts: DateTime<Utc>) -> String {
    ts.format("%H:%M").to_string()
}

/// Renewable coverage (gen ÷ load, %), uncapped — the "educk curve".
fn coverage_raw(p: &RenewableSurplus) -> f64 {
    if p.load > 0.0 {
        p.generation / p.load * 100.0
    } else {
        0.0
    }
}

/// 3-hour generation trend around `now`, with a first/second-half fallback.
fn renewable_trend(
    series: &[RenewableSurplus],
    now: DateTime<Utc>,
    t: &i18n::Strings,
) -> &'static str {
    if series.len() < 4 {
        return t.trend_steady;
    }
    let w = Duration::hours(3);
    let avg = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
    let past: Vec<f64> = series
        .iter()
        .filter(|p| p.timestamp < now && p.timestamp > now - w)
        .map(|p| p.generation)
        .collect();
    let future: Vec<f64> = series
        .iter()
        .filter(|p| p.timestamp > now && p.timestamp < now + w)
        .map(|p| p.generation)
        .collect();
    let (pa, fa) = if past.is_empty() || future.is_empty() {
        let mid = series.len() / 2;
        (
            avg(&series[..mid].iter().map(|p| p.generation).collect::<Vec<_>>()),
            avg(&series[mid..].iter().map(|p| p.generation).collect::<Vec<_>>()),
        )
    } else {
        (avg(&past), avg(&future))
    };
    if pa <= 0.0 {
        return if fa > 0.0 { t.trend_rising } else { t.trend_steady };
    }
    let ch = (fa - pa) / pa;
    if ch > 0.05 {
        t.trend_rising
    } else if ch < -0.05 {
        t.trend_falling
    } else {
        t.trend_steady
    }
}

/// Build the inner SVG for #gauge (mirrors app.js `renderGauge`).
fn gauge_svg(value: f64, coverage: f64, t: &i18n::Strings) -> String {
    let (cx, cy, r, sw) = (100.0_f64, 92.0_f64, 76.0_f64, 13.0_f64);
    let color = gauge_color(value);
    let track = gauge_arc_path(cx, cy, r, GAUGE_START, GAUGE_SWEEP);
    let (tx, ty) = gauge_polar(cx, cy, r, GAUGE_START + GAUGE_SWEEP * value);
    let fill = if value > 0.01 {
        format!(
            "<path d=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{sw}\" stroke-linecap=\"round\"/>\
             <circle cx=\"{tx:.2}\" cy=\"{ty:.2}\" r=\"{:.2}\" fill=\"#fff\" stroke=\"{color}\" stroke-width=\"2\"/>",
            gauge_arc_path(cx, cy, r, GAUGE_START, GAUGE_SWEEP * value),
            sw * 0.42,
        )
    } else {
        String::new()
    };
    format!(
        "<svg viewBox=\"0 0 200 150\" class=\"w-full\" role=\"img\" aria-label=\"{:.0}% {}\">\
         <path d=\"{track}\" fill=\"none\" stroke=\"#e2e8f0\" stroke-width=\"{sw}\" stroke-linecap=\"round\"/>{fill}\
         <text x=\"{cx}\" y=\"{:.0}\" text-anchor=\"middle\" font-size=\"40\" font-weight=\"700\" fill=\"#0f172a\" letter-spacing=\"-1.5\">{:.0}%</text>\
         <text x=\"{cx}\" y=\"{:.0}\" text-anchor=\"middle\" font-size=\"11\" font-weight=\"500\" fill=\"#94a3b8\" letter-spacing=\"0.4\">{}</text>\
         </svg>",
        coverage, t.gauge_renewable_now, cy - 4.0, coverage, cy + 16.0, t.gauge_renewable_now,
    )
}

/// Build the inner SVG for #chart: the "educk curve" overview (renewable
/// coverage % over time). Mirrors app.js `renderOverviewChart`; rendered at a
/// fixed 700×260 coordinate space and stretched responsively, then replaced by
/// the interactive version on hydrate. `preserveAspectRatio="none"` keeps the
/// slot height constant (no layout shift) across viewport widths.
fn overview_chart_svg(series: &[RenewableSurplus], now: DateTime<Utc>, t: &i18n::Strings) -> String {
    let n = series.len();
    if n == 0 {
        return String::new();
    }
    let (w, h) = (700.0_f64, 260.0_f64);
    let (pad_t, pad_r, pad_b, pad_l) = (10.0_f64, 8.0_f64, 26.0_f64, 54.0_f64);
    let plot_w = w - pad_l - pad_r;
    let plot_h = h - pad_t - pad_b;

    let cov: Vec<f64> = series.iter().map(coverage_raw).collect();
    let max_c = cov.iter().copied().fold(0.0_f64, f64::max);
    let max_y = 100.0_f64.max(max_c) * 1.08;

    let x_at = |i: f64| pad_l + if n == 1 { plot_w / 2.0 } else { (i / (n as f64 - 1.0)) * plot_w };
    let y_at = |v: f64| pad_t + (1.0 - v / max_y) * plot_h;
    let base_y = y_at(0.0);

    let line_pts: Vec<String> = cov
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{:.1},{:.1}", x_at(i as f64), y_at(*c)))
        .collect();
    let line_pts = line_pts.join(" ");
    let area_pts = format!(
        "{:.1},{:.1} {line_pts} {:.1},{:.1}",
        x_at(0.0),
        base_y,
        x_at(n as f64 - 1.0),
        base_y
    );

    // Y grid every 25%, the 100% ("fully renewable") line emphasised.
    let mut y_ticks = String::new();
    let mut v = 0.0;
    while v <= max_y {
        let y = y_at(v);
        let is100 = (v - 100.0).abs() < 0.01;
        let (stroke, dash) = if is100 {
            ("#94a3b8", " stroke-dasharray=\"4 3\"")
        } else {
            ("#e2e8f0", "")
        };
        y_ticks.push_str(&format!(
            "<line x1=\"{pad_l}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"{stroke}\" stroke-width=\"1\"{dash}/>\
             <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" font-size=\"10\" fill=\"#94a3b8\">{}%</text>",
            w - pad_r, pad_l - 6.0, y + 3.0, v as i64
        ));
        v += 25.0;
    }

    // "Now" marker, interpolated between bracketing points.
    let now_x: Option<f64> = if series[0].timestamp > now {
        Some(x_at(0.0))
    } else if series[n - 1].timestamp < now {
        Some(x_at(n as f64 - 1.0))
    } else {
        let mut res = None;
        for i in 1..n {
            if series[i].timestamp >= now {
                let span = (series[i].timestamp - series[i - 1].timestamp).num_seconds() as f64;
                let f = if span > 0.0 {
                    (now - series[i - 1].timestamp).num_seconds() as f64 / span
                } else {
                    0.0
                };
                res = Some(x_at(i as f64 - 1.0 + f));
                break;
            }
        }
        res
    };
    let now_marker = now_x
        .map(|nx| {
            format!(
                "<line x1=\"{nx:.1}\" y1=\"{pad_t}\" x2=\"{nx:.1}\" y2=\"{:.1}\" stroke=\"#ea580c\" stroke-width=\"1.5\" stroke-dasharray=\"6 3\"/>\
                 <text x=\"{nx:.1}\" y=\"{:.0}\" text-anchor=\"middle\" font-size=\"10\" font-weight=\"600\" fill=\"#c2410c\">{}</text>",
                h - pad_b, pad_t + 9.0, t.chart_now
            )
        })
        .unwrap_or_default();

    // X axis time labels, ~8 across.
    let mut x_labels = String::new();
    let every = ((n as f64) / 8.0).ceil().max(1.0) as usize;
    let mut i = 0;
    while i < n {
        x_labels.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"10\" fill=\"#94a3b8\">{}</text>",
            x_at(i as f64),
            h - pad_b + 16.0,
            fmt_hm(series[i].timestamp)
        ));
        i += every;
    }

    format!(
        "<svg viewBox=\"0 0 {w:.0} {h:.0}\" width=\"100%\" height=\"{h:.0}\" preserveAspectRatio=\"none\" style=\"display:block\">\
         <defs><linearGradient id=\"curve-fill\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\
         <stop offset=\"0%\" stop-color=\"#059669\" stop-opacity=\"0.5\"/>\
         <stop offset=\"100%\" stop-color=\"#059669\" stop-opacity=\"0.04\"/></linearGradient></defs>\
         {y_ticks}<polygon points=\"{area_pts}\" fill=\"url(#curve-fill)\"/>\
         <polyline points=\"{line_pts}\" fill=\"none\" stroke=\"#059669\" stroke-width=\"2.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\
         {now_marker}{x_labels}</svg>"
    )
}

/// Build the server-rendered dashboard snapshot from a cached series.
fn build_dashboard(
    series: &[RenewableSurplus],
    country_code: &str,
    now: DateTime<Utc>,
    t: &i18n::Strings,
) -> Option<DashboardView> {
    let cur = series
        .iter()
        .min_by_key(|p| (p.timestamp - now).num_seconds().abs())?;
    let peak = series.iter().max_by(|a, b| a.surplus.total_cmp(&b.surplus))?;

    // Gauge needle: 0 = worst surplus of the window, 1 = best.
    let (lo, hi) = series.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.surplus), hi.max(p.surplus))
    });
    let value = if hi > lo {
        ((cur.surplus - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let coverage = cur.renewable_share().min(100.0);

    let surplus = cur.surplus >= 0.0;
    let (cur_bg, cur_border, cur_color) = if surplus {
        ("#ecfdf5", "#a7f3d0", "#047857")
    } else {
        ("#fef2f2", "#fecaca", "#b91c1c")
    };

    let cov_color = if coverage >= 80.0 {
        "#15803d"
    } else if coverage >= 50.0 {
        "#4d7c0f"
    } else if coverage >= 30.0 {
        "#b45309"
    } else {
        "#b91c1c"
    };

    let peak_good = peak.surplus > 0.0;
    let (hint_bg, hint_border) = if peak_good {
        ("#ecfdf5", "#a7f3d0")
    } else {
        ("#fffbeb", "#fde68a")
    };
    let hint_template = if peak_good { t.hint_good } else { t.hint_bad };
    let hint_text = hint_template.replace("{time}", &fmt_hm(peak.timestamp));

    let range_text = format!(
        "{}, {} → {} ({} {})",
        series[0].timestamp.format("%a %d %b"),
        fmt_hm(series[0].timestamp),
        fmt_hm(series[series.len() - 1].timestamp),
        series.len(),
        t.chart_data_points,
    );

    let data_json = json!({
        "country": country_code,
        "timestamps": series.iter().map(|s| s.timestamp.to_rfc3339()).collect::<Vec<_>>(),
        "generation": series.iter().map(|s| s.generation).collect::<Vec<_>>(),
        "load": series.iter().map(|s| s.load).collect::<Vec<_>>(),
        "surplus": series.iter().map(|s| s.surplus).collect::<Vec<_>>(),
    })
    .to_string();

    Some(DashboardView {
        data_json,
        gauge_html: gauge_svg(value, coverage, t),
        current_title: if surplus { t.card_surplus_now } else { t.card_deficit_now },
        current_value: fmt_mw(cur.surplus.abs()),
        current_sub: format!("{:.0}% {}", cur.surplus_percentage(), t.card_of_generation),
        current_trend: renewable_trend(series, now, t),
        current_card_style: format!("background:{cur_bg};border-color:{cur_border}"),
        current_value_style: format!("color:{cur_color}"),
        peak_time: fmt_hm(peak.timestamp),
        peak_value: fmt_mw(peak.surplus),
        coverage_value: format!("{:.0}%", coverage),
        coverage_value_style: format!("color:{cov_color}"),
        coverage_card_style: format!("background:{cov_color}14;border-color:{cov_color}40"),
        chart_svg: overview_chart_svg(series, now, t),
        range_text,
        hint_text,
        hint_style: format!("background:{hint_bg};border-color:{hint_border}"),
    })
}

#[derive(Template)]
#[template(path = "index.html")]
struct LandingTemplate {
    t: &'static i18n::Strings,
    title: String,
    meta_description: String,
    canonical_url: String,
    alt_en: String,
    alt_de: String,
    json_ld: String,
    home_url: String,
    impressum_url: String,
    privacy_url: String,
    support_url: String,
    dashboard: Option<DashboardView>,
    countries: Vec<CountryLink>,
    cloud_providers: Vec<CloudProviderGroup>,
}

#[derive(Deserialize)]
struct LandingQuery {
    lang: Option<String>,
}

/// GET /
/// The web frontend shell: the interactive dashboard (gauge + cards + chart) is
/// rendered client-side by `/app/app.js` against the same-origin `/api/v1`, with
/// the server-rendered country/cloud index below for crawlers. This handler makes
/// no upstream ENTSO-E call — it only emits the static, crawlable surface.
async fn get_landing(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: OriginalUri,
    Query(query): Query<LandingQuery>,
) -> Result<Response, StatusCode> {
    let (lang, persist) = match resolve_lang(&headers, &uri, query.lang.as_deref()) {
        Ok(v) => v,
        Err(redirect) => return Ok(redirect),
    };
    let t = i18n::strings(lang);

    let mut countries: Vec<CountryLink> = areas::list_countries()
        .into_iter()
        .filter_map(|code| {
            get_primary_zone(code).map(|zone| CountryLink {
                name: i18n::country_name(zone.name, lang),
                url: i18n::localize_url(&format!("/electricity/{}", code.to_lowercase()), lang),
            })
        })
        .collect();
    countries.sort_by(|a, b| a.name.cmp(&b.name));

    // First-paint dashboard for the default country (DE), but only from already
    // warm cache — `/` must never block on ENTSO-E. On a cold cache this is None
    // and the client dashboard fetches on its own (spinner fallback).
    let dashboard = get_primary_zone("DE").and_then(|z| {
        state
            .cached_series(z.code)
            .and_then(|series| build_dashboard(&series, z.country_code, Utc::now(), t))
    });

    // Cloud regions grouped by provider. `all_regions()` is sorted by
    // (provider, region), so consecutive grouping preserves that order.
    let mut cloud_providers: Vec<CloudProviderGroup> = Vec::new();
    for cr in cloud::all_regions() {
        let label = provider_label(cr.provider).to_string();
        let link = CloudLink {
            region: cr.region.to_string(),
            location: cr.location.to_string(),
            url: i18n::localize_url(&format!("/cloud/{}/{}", cr.provider, cr.region), lang),
        };
        match cloud_providers.last_mut() {
            Some(group) if group.provider_label == label => group.regions.push(link),
            _ => cloud_providers.push(CloudProviderGroup {
                provider_label: label,
                regions: vec![link],
            }),
        }
    }

    let title = i18n::index_title(lang).to_string();
    let meta_description = i18n::index_meta(lang).to_string();
    let (canonical_url, alt_en, alt_de) = i18n::page_urls(&state.base_url, "/", lang);

    let list_items: Vec<_> = countries
        .iter()
        .enumerate()
        .map(|(i, c)| {
            json!({
                "@type": "ListItem",
                "position": i + 1,
                "name": i18n::country_about(lang, &c.name),
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
                "inLanguage": lang.code(),
            },
            {
                "@type": "ItemList",
                "name": i18n::itemlist_name(lang),
                "itemListElement": list_items,
            }
        ],
    })
    .to_string();

    let template = LandingTemplate {
        t,
        title,
        meta_description,
        canonical_url,
        alt_en,
        alt_de,
        json_ld,
        home_url: i18n::localize_url("/", lang),
        impressum_url: i18n::localize_url("/impressum", lang),
        privacy_url: i18n::localize_url("/privacy", lang),
        support_url: i18n::localize_url("/support", lang),
        dashboard,
        countries,
        cloud_providers,
    };

    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(with_lang_cookie(Html(html).into_response(), lang, persist))
}

#[derive(Template)]
#[template(path = "cloud.html")]
struct CloudPageTemplate {
    t: &'static i18n::Strings,
    title: String,
    meta_description: String,
    canonical_url: String,
    alt_en: String,
    alt_de: String,
    json_ld: String,
    home_url: String,
    impressum_url: String,
    privacy_url: String,
    support_url: String,
    h1: String,
    lead: String,
    hourly_h2: String,
    cta_label: String,
    table_caption: String,
    country_url: String,
    updated_utc: String,
    sentences: Vec<String>,
    rows: Vec<ForecastRow>,
    plot_data: String,
    plot_layout: String,
}

/// GET /cloud/{provider}/{region}
/// Server-rendered, crawlable forecast page for a cloud region, framed for
/// carbon-aware workload scheduling. Uses the region's underlying national grid.
async fn get_cloud_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: OriginalUri,
    Path((provider, region)): Path<(String, String)>,
    Query(q): Query<LangQuery>,
) -> Result<Response, StatusCode> {
    let cr = cloud::lookup(&provider, &region).ok_or(StatusCode::NOT_FOUND)?;
    let (lang, persist) = match resolve_lang(&headers, &uri, q.lang.as_deref()) {
        Ok(v) => v,
        Err(redirect) => return Ok(redirect),
    };
    let t = i18n::strings(lang);
    let zone = get_primary_zone(cr.country_code).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let country_name = i18n::country_name(zone.name, lang);

    let now = Utc::now();
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    let summary = summarize_forecast(&series, &country_name, lang);
    let rows = forecast_rows(&series);
    let (plot_data, plot_layout) = generate_plot_data(&series, None);

    let label = provider_label(cr.provider);
    let title = i18n::cloud_title(lang, cr.provider, cr.region, cr.location);
    let meta_description = summary.meta_description(&country_name, lang);
    let path = format!("/cloud/{}/{}", cr.provider, cr.region);
    let (canonical_url, alt_en, alt_de) = i18n::page_urls(&state.base_url, &path, lang);
    let updated_utc = now.format("%Y-%m-%d %H:%M UTC").to_string();

    let json_ld = json!({
        "@context": "https://schema.org",
        "@type": "WebPage",
        "name": title,
        "description": meta_description,
        "url": canonical_url,
        "inLanguage": lang.code(),
        "dateModified": now.to_rfc3339(),
        "isPartOf": {
            "@type": "WebSite",
            "name": "educk",
            "url": state.base_url.to_string(),
        },
        "about": {
            "@type": "Thing",
            "name": i18n::cloud_about(lang, label, cr.region),
        },
    })
    .to_string();

    let template = CloudPageTemplate {
        t,
        title,
        meta_description,
        canonical_url,
        alt_en,
        alt_de,
        json_ld,
        home_url: i18n::localize_url("/", lang),
        impressum_url: i18n::localize_url("/impressum", lang),
        privacy_url: i18n::localize_url("/privacy", lang),
        support_url: i18n::localize_url("/support", lang),
        h1: i18n::cloud_h1(lang, label, cr.region),
        lead: i18n::cloud_lead(lang, label, cr.region, cr.location, &country_name),
        hourly_h2: i18n::cloud_hourly_h2(lang, label, cr.region),
        cta_label: i18n::cloud_cta(lang, &country_name),
        table_caption: i18n::caption_cloud(lang, &country_name),
        country_url: i18n::localize_url(
            &format!("/electricity/{}", cr.country_code.to_lowercase()),
            lang,
        ),
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

    Ok(with_lang_cookie(Html(html).into_response(), lang, persist))
}

// ── Legal pages (Impressum / privacy policy) + support ───────────────────────

#[derive(Template)]
#[template(path = "impressum.html")]
struct ImpressumTemplate {
    t: &'static i18n::Strings,
    canonical_url: String,
    alt_en: String,
    alt_de: String,
    home_url: String,
    impressum_url: String,
    privacy_url: String,
    support_url: String,
}

/// GET /impressum
/// Static legal notice (Impressum) required of a German operator under § 5 DDG.
async fn get_impressum(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: OriginalUri,
    Query(q): Query<LangQuery>,
) -> Result<Response, StatusCode> {
    let (lang, persist) = match resolve_lang(&headers, &uri, q.lang.as_deref()) {
        Ok(v) => v,
        Err(redirect) => return Ok(redirect),
    };
    let (canonical_url, alt_en, alt_de) = i18n::page_urls(&state.base_url, "/impressum", lang);
    let template = ImpressumTemplate {
        t: i18n::strings(lang),
        canonical_url,
        alt_en,
        alt_de,
        home_url: i18n::localize_url("/", lang),
        impressum_url: i18n::localize_url("/impressum", lang),
        privacy_url: i18n::localize_url("/privacy", lang),
        support_url: i18n::localize_url("/support", lang),
    };
    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(with_lang_cookie(Html(html).into_response(), lang, persist))
}

#[derive(Template)]
#[template(path = "privacy.html")]
struct PrivacyTemplate {
    t: &'static i18n::Strings,
    canonical_url: String,
    alt_en: String,
    alt_de: String,
    home_url: String,
    impressum_url: String,
    privacy_url: String,
    support_url: String,
}

/// GET /privacy
/// Static GDPR privacy policy (Datenschutzerklärung); discloses Google Analytics.
async fn get_privacy(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: OriginalUri,
    Query(q): Query<LangQuery>,
) -> Result<Response, StatusCode> {
    let (lang, persist) = match resolve_lang(&headers, &uri, q.lang.as_deref()) {
        Ok(v) => v,
        Err(redirect) => return Ok(redirect),
    };
    let (canonical_url, alt_en, alt_de) = i18n::page_urls(&state.base_url, "/privacy", lang);
    let template = PrivacyTemplate {
        t: i18n::strings(lang),
        canonical_url,
        alt_en,
        alt_de,
        home_url: i18n::localize_url("/", lang),
        impressum_url: i18n::localize_url("/impressum", lang),
        privacy_url: i18n::localize_url("/privacy", lang),
        support_url: i18n::localize_url("/support", lang),
    };
    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(with_lang_cookie(Html(html).into_response(), lang, persist))
}

#[derive(Template)]
#[template(path = "support.html")]
struct SupportTemplate {
    t: &'static i18n::Strings,
    canonical_url: String,
    alt_en: String,
    alt_de: String,
    home_url: String,
    impressum_url: String,
    privacy_url: String,
    support_url: String,
}

/// GET /support
/// Contact page: how to report a problem, flag a data issue, or suggest an
/// improvement. Everything routes to info@vectorandveneer.com — there is no form
/// (and therefore no form data to process).
async fn get_support(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: OriginalUri,
    Query(q): Query<LangQuery>,
) -> Result<Response, StatusCode> {
    let (lang, persist) = match resolve_lang(&headers, &uri, q.lang.as_deref()) {
        Ok(v) => v,
        Err(redirect) => return Ok(redirect),
    };
    let (canonical_url, alt_en, alt_de) = i18n::page_urls(&state.base_url, "/support", lang);
    let template = SupportTemplate {
        t: i18n::strings(lang),
        canonical_url,
        alt_en,
        alt_de,
        home_url: i18n::localize_url("/", lang),
        impressum_url: i18n::localize_url("/impressum", lang),
        privacy_url: i18n::localize_url("/privacy", lang),
        support_url: i18n::localize_url("/support", lang),
    };
    let html = template.render().map_err(|e| {
        tracing::error!("Template rendering error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(with_lang_cookie(Html(html).into_response(), lang, persist))
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

    // Cached, stale-on-error; degrades to an empty series when upstream is down.
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();
    let series = filter_next_hours(series, hours);

    let Some(best) = find_max(series) else {
        tracing::warn!("no surplus data for {provider}/{region}");
        return Ok(Json(ApiResponse::error(
            "Forecast data is temporarily unavailable. Please try again shortly.".to_string(),
        )));
    };

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

// ── Modelled proxy debug endpoint ────────────────────────────────────────────

#[derive(Serialize)]
struct ModelledDebugResponse {
    grid_generated: String,
    grid_cells: usize,
    solar_capture_frac: f64,
    wind_capture_frac: f64,
    points: Vec<ModelledDebugPoint>,
}

#[derive(Serialize)]
struct ModelledDebugPoint {
    timestamp: String,
    entsoe_generation_mw: Option<f64>,
    modelled_total_mw: f64,
    modelled_solar_mw: f64,
    modelled_wind_mw: f64,
}

/// GET /api/v1/de/modelled
/// Debug view: the modelled (MaStR × weather) German series next to the ENTSO-E
/// wind+solar forecast, for eyeballing the fit and inspecting grid coverage.
async fn get_de_modelled(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ModelledDebugResponse>>, StatusCode> {
    let zone = get_primary_zone("DE").ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let series = state.series_for_zone(zone.code).await.unwrap_or_default();

    let Some(modelled) = state.modelled_de(&series).await else {
        return Ok(Json(ApiResponse::error(
            "No modelled series — empty capacity grid (run mastr_ingest) or weather fetch failed."
                .to_string(),
        )));
    };

    let truth: HashMap<_, _> = series.iter().map(|s| (s.timestamp, s.generation)).collect();
    let grid = &*crate::grid::DE_CAPACITY_GRID;
    let points = modelled
        .iter()
        .map(|m| ModelledDebugPoint {
            timestamp: m.timestamp.to_rfc3339(),
            entsoe_generation_mw: truth.get(&m.timestamp).copied(),
            modelled_total_mw: m.total_mw,
            modelled_solar_mw: m.solar_mw,
            modelled_wind_mw: m.wind_mw,
        })
        .collect();

    Ok(Json(ApiResponse::success(ModelledDebugResponse {
        grid_generated: grid.generated.clone(),
        grid_cells: grid.cells.len(),
        solar_capture_frac: grid.solar_capture_frac,
        wind_capture_frac: grid.wind_capture_frac,
        points,
    })))
}

/// GET /health
async fn health() -> &'static str {
    "OK"
}

// Brand assets embedded in the binary so the SSR site is self-contained (no
// runtime file dependency or volume mount). The duck logo and its derived
// favicons are the source-of-truth files in `static/`.
const LOGO_SVG: &[u8] = include_bytes!("../static/logo_bw.svg");
const LOGO_PNG: &[u8] = include_bytes!("../static/logo_bw.png");
const FAVICON_ICO: &[u8] = include_bytes!("../static/favicon.ico");
const FAVICON_PNG: &[u8] = include_bytes!("../static/favicon-32.png");
const APPLE_TOUCH_ICON: &[u8] = include_bytes!("../static/apple-touch-icon.png");

/// Serve an embedded asset with a long cache lifetime; these change rarely.
fn asset(content_type: &'static str, bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=604800, immutable"),
        ],
        bytes,
    )
}

async fn favicon_ico() -> impl IntoResponse {
    asset("image/x-icon", FAVICON_ICO)
}
async fn favicon_png() -> impl IntoResponse {
    asset("image/png", FAVICON_PNG)
}
async fn apple_touch_icon() -> impl IntoResponse {
    asset("image/png", APPLE_TOUCH_ICON)
}
async fn logo_svg() -> impl IntoResponse {
    asset("image/svg+xml", LOGO_SVG)
}
async fn logo_png() -> impl IntoResponse {
    asset("image/png", LOGO_PNG)
}

// ── Lightweight dashboard (vanilla HTML/JS/SVG, served at /app) ────────────────
// Replaces the heavy Flutter web bundle. Files are embedded so the binary stays
// self-contained; the JS calls the same-origin /api/v1 endpoints. app.css is the
// committed Tailwind build (see `just build-css`).
const APP_HTML: &[u8] = include_bytes!("../static/app/index.html");
const APP_JS: &[u8] = include_bytes!("../static/app/app.js");
const APP_CSS: &[u8] = include_bytes!("../static/app/app.css");

/// Serve an embedded dashboard asset. Unlike the brand assets these change every
/// deploy and are not content-hashed, so they revalidate rather than cache hard.
fn app_asset(content_type: &'static str, bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
}

async fn app_index() -> impl IntoResponse {
    app_asset("text/html; charset=utf-8", APP_HTML)
}
async fn app_js() -> impl IntoResponse {
    app_asset("application/javascript; charset=utf-8", APP_JS)
}
async fn app_css() -> impl IntoResponse {
    app_asset("text/css; charset=utf-8", APP_CSS)
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
/// Each page exists in English (the bare path, also `x-default`) and German
/// (`?lang=de`); both are emitted as separate `<url>`s carrying the reciprocal
/// `hreflang` alternates so search engines surface the right language version.
async fn sitemap_xml(State(state): State<AppState>) -> impl IntoResponse {
    let base = state.base_url.as_ref();
    let entry = |path: &str| -> String {
        let en = format!("{base}{path}");
        let de = format!("{base}{}", i18n::add_lang_param(path, "de"));
        let alts = format!(
            "    <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"{en}\"/>\n\
             \x20   <xhtml:link rel=\"alternate\" hreflang=\"de\" href=\"{de}\"/>\n\
             \x20   <xhtml:link rel=\"alternate\" hreflang=\"x-default\" href=\"{en}\"/>\n"
        );
        format!("  <url><loc>{en}</loc>\n{alts}  </url>\n  <url><loc>{de}</loc>\n{alts}  </url>\n")
    };

    let mut urls = entry("/");
    urls.push_str(&entry("/support"));
    for code in areas::list_countries() {
        urls.push_str(&entry(&format!("/electricity/{}", code.to_lowercase())));
    }
    for cr in cloud::all_regions() {
        urls.push_str(&entry(&format!("/cloud/{}/{}", cr.provider, cr.region)));
    }
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" \
         xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n\
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
        weather_client: Arc::new(WeatherClient::new()),
        base_url: Arc::from(config.public_base_url.trim_end_matches('/')),
        series_cache: Arc::new(Mutex::new(HashMap::new())),
        modelled_cache: Arc::new(Mutex::new(None)),
    };

    // Proactively warm the caches so the landing page is always fast.
    let warmer = state.clone();
    tokio::spawn(async move {
        loop {
            for want_cache in ["DE", "PT"] {
                if let Some(zone) = get_primary_zone(want_cache) {
                    if let Err(e) = warmer.series_for_zone(zone.code).await {
                        tracing::warn!("{} cache warm-up failed: {:?}", want_cache, e);
                    } else {
                        tracing::debug!("{} cache refreshed", want_cache);
                    }
                }
            }
            tokio::time::sleep(CACHE_TTL).await;
        }
    });

    let app = Router::new()
        .route("/", get(get_landing))
        .route("/health", get(health))
        .route("/robots.txt", get(robots_txt))
        .route("/sitemap.xml", get(sitemap_xml))
        .route("/favicon.ico", get(favicon_ico))
        .route("/static/favicon-32.png", get(favicon_png))
        .route("/static/apple-touch-icon.png", get(apple_touch_icon))
        .route("/static/logo_bw.svg", get(logo_svg))
        .route("/static/logo_bw.png", get(logo_png))
        .route("/app", get(app_index))
        .route("/app/", get(app_index))
        .route("/app/app.js", get(app_js))
        .route("/app/app.css", get(app_css))
        .route("/electricity/{country}", get(get_country_page))
        .route("/cloud/{provider}/{region}", get(get_cloud_page))
        .route("/impressum", get(get_impressum))
        .route("/privacy", get(get_privacy))
        .route("/support", get(get_support))
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
        .route("/api/v1/de/modelled", get(get_de_modelled))
        .route("/api/v1/cloud/regions", get(list_cloud_regions))
        .route(
            "/api/v1/cloud/{provider}/{region}/next",
            get(get_cloud_best_window),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        // Compress SSR HTML, JSON and the dashboard JS/CSS (gzip/brotli). This is
        // the front door now that nginx no longer sits in front of the service.
        .layer(CompressionLayer::new())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3044").await?;
    tracing::info!("server listening on http://0.0.0.0:3044");

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(k.clone(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn negotiate_explicit_query_wins_and_persists() {
        // An explicit ?lang= beats everything and is remembered, regardless of the
        // Accept-Language header.
        let h = headers(&[(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")]);
        assert!(matches!(
            negotiate_lang(&h, Some("de")),
            LangChoice::Render {
                lang: Lang::De,
                persist: true
            }
        ));
        assert!(matches!(
            negotiate_lang(&h, Some("en")),
            LangChoice::Render {
                lang: Lang::En,
                persist: true
            }
        ));
    }

    #[test]
    fn negotiate_cookie_then_accept_language() {
        // Remembered German choice -> redirect to the ?lang=de variant.
        let de_cookie = headers(&[(header::COOKIE, "educk_lang=de; other=1")]);
        assert!(matches!(
            negotiate_lang(&de_cookie, None),
            LangChoice::RedirectDe
        ));
        // Remembered English choice -> stay English even if the browser prefers de.
        let en_cookie = headers(&[
            (header::COOKIE, "educk_lang=en"),
            (header::ACCEPT_LANGUAGE, "de-DE,de;q=0.9"),
        ]);
        assert!(matches!(
            negotiate_lang(&en_cookie, None),
            LangChoice::Render {
                lang: Lang::En,
                persist: false
            }
        ));
        // No cookie, German browser -> soft redirect.
        let de_browser = headers(&[(header::ACCEPT_LANGUAGE, "de-DE,de;q=0.9,en;q=0.8")]);
        assert!(matches!(
            negotiate_lang(&de_browser, None),
            LangChoice::RedirectDe
        ));
        // No cookie, English browser (the crawler path) -> English, no redirect.
        let en_browser = headers(&[(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")]);
        assert!(matches!(
            negotiate_lang(&en_browser, None),
            LangChoice::Render {
                lang: Lang::En,
                persist: false
            }
        ));
        // No headers at all (e.g. a bare bot) -> English.
        assert!(matches!(
            negotiate_lang(&HeaderMap::new(), None),
            LangChoice::Render {
                lang: Lang::En,
                persist: false
            }
        ));
    }

    /// Build a minimal Impressum template in the given language. The Impressum page
    /// exercises the bilingual body switch, the hreflang block, the language
    /// switcher and the localized cookie-consent include.
    fn impressum(lang: Lang) -> ImpressumTemplate {
        let (canonical_url, alt_en, alt_de) = i18n::page_urls("https://educk.io", "/impressum", lang);
        ImpressumTemplate {
            t: i18n::strings(lang),
            canonical_url,
            alt_en,
            alt_de,
            home_url: i18n::localize_url("/", lang),
            impressum_url: i18n::localize_url("/impressum", lang),
            privacy_url: i18n::localize_url("/privacy", lang),
            support_url: i18n::localize_url("/support", lang),
        }
    }

    /// Build a minimal Support template in the given language.
    fn support(lang: Lang) -> SupportTemplate {
        let (canonical_url, alt_en, alt_de) = i18n::page_urls("https://educk.io", "/support", lang);
        SupportTemplate {
            t: i18n::strings(lang),
            canonical_url,
            alt_en,
            alt_de,
            home_url: i18n::localize_url("/", lang),
            impressum_url: i18n::localize_url("/impressum", lang),
            privacy_url: i18n::localize_url("/privacy", lang),
            support_url: i18n::localize_url("/support", lang),
        }
    }

    #[test]
    fn renders_support_page_in_both_languages() {
        let en = support(Lang::En).render().unwrap();
        assert!(en.contains("<html lang=\"en\">"));
        assert!(en.contains("Report a problem"));
        assert!(en.contains("Ideas for improvement"));
        assert!(en.contains("mailto:info@vectorandveneer.com"));
        assert!(en.contains("<link rel=\"canonical\" href=\"https://educk.io/support\">"));

        let de = support(Lang::De).render().unwrap();
        assert!(de.contains("<html lang=\"de\">"));
        assert!(de.contains("Ein Problem melden"));
        assert!(de.contains("mailto:info@vectorandveneer.com"));
        assert!(de.contains("<link rel=\"canonical\" href=\"https://educk.io/support?lang=de\">"));
        // Footer/nav links stay in-language on the German page.
        assert!(de.contains("href=\"/impressum?lang=de\""));
    }

    #[test]
    fn renders_german_page_with_hreflang() {
        let html = impressum(Lang::De).render().unwrap();
        assert!(html.contains("<html lang=\"de\">"));
        assert!(html.contains("Diensteanbieter")); // German body
        assert!(html.contains("Ablehnen")); // localized cookie button
        // Reciprocal hreflang alternates + self-canonical at the ?lang=de URL.
        assert!(html.contains("hreflang=\"de\" href=\"https://educk.io/impressum?lang=de\""));
        assert!(html.contains("hreflang=\"en\" href=\"https://educk.io/impressum\""));
        assert!(html.contains("hreflang=\"x-default\" href=\"https://educk.io/impressum\""));
        assert!(html.contains("<link rel=\"canonical\" href=\"https://educk.io/impressum?lang=de\">"));
    }

    #[test]
    fn renders_english_page_by_default() {
        let html = impressum(Lang::En).render().unwrap();
        assert!(html.contains("<html lang=\"en\">"));
        assert!(html.contains("Service provider")); // English body
        assert!(html.contains("Decline"));
        assert!(html.contains("<link rel=\"canonical\" href=\"https://educk.io/impressum\">"));
    }
}
