//! The daily Instagram Story: a 1080×1920 card for Germany showing how much of
//! the day's electricity demand wind and solar cover, hour by hour, plus a
//! concrete recommendation for when to run flexible loads.
//!
//! Rendering is SVG → `resvg` → JPEG (the only image format Instagram accepts).
//! Inter is embedded so the slim runtime image needs no system fonts. All times
//! shown are German wall-clock time (CET/CEST).

use std::sync::Arc;

use chrono::{
    DateTime, Datelike, Duration, DurationRound, NaiveDate, NaiveTime, TimeZone, Timelike, Utc,
};
use chrono_tz::Europe::Berlin;
use once_cell::sync::Lazy;
use resvg::{tiny_skia, usvg};

use crate::entsoe::analysis::RenewableSurplus;

pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 1920;
const JPEG_QUALITY: u8 = 90;

/// Length of the recommended "best time" (and "avoid") window.
const WINDOW_HOURS: usize = 3;
/// Fewer hours than this left in the day → the story covers the next day instead.
const MIN_HOURS: usize = 6;
/// Below this gap (percentage points) between the best and the worst window,
/// timing barely matters and the story says so instead of naming an "avoid" slot.
const MIN_SPREAD: f64 = 10.0;
/// A worst window at least this green is not worth avoiding.
const GREEN_ANYWAY: f64 = 80.0;

// Text tokens — the same slate scale the site uses.
const INK: &str = "#0f172a";
const INK_2: &str = "#475569";
const MUTED: &str = "#94a3b8";
const GRID: &str = "#e2e8f0";
// Data + status colors.
const GREEN: &str = "#059669";
const AMBER: &str = "#d97706";

static FONTS: Lazy<Arc<usvg::fontdb::Database>> = Lazy::new(|| {
    let mut db = usvg::fontdb::Database::new();
    for data in [
        &include_bytes!("../static/fonts/Inter-Regular.ttf")[..],
        &include_bytes!("../static/fonts/Inter-SemiBold.ttf")[..],
        &include_bytes!("../static/fonts/Inter-Bold.ttf")[..],
    ] {
        db.load_font_data(data.to_vec());
    }
    db.set_sans_serif_family("Inter");
    Arc::new(db)
});

/// One hour of the forecast (hourly totals of the 15/60-min ENTSO-E points).
#[derive(Debug, Clone, Copy)]
struct Hour {
    start: DateTime<Utc>,
    generation: f64,
    load: f64,
}

/// A contiguous run of hours and the share of demand wind + solar cover in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Wind + solar as % of load over the window (energy-weighted, uncapped).
    pub coverage: f64,
}

/// Everything the story says about one German calendar day.
#[derive(Debug, Clone)]
pub struct Outlook {
    pub day: NaiveDate,
    /// Raw points (timestamp, coverage %) for the curve.
    points: Vec<(DateTime<Utc>, f64)>,
    /// Hour with the highest coverage.
    pub peak: Window,
    pub best: Window,
    pub worst: Window,
}

/// What the second card says about the day's weakest window.
#[derive(Debug, PartialEq)]
pub enum Weakest {
    /// Clearly less green than the best window: shift loads out of it.
    Avoid,
    /// Noticeably weaker, but still mostly wind + solar.
    StillGreen,
    /// About as green as the best window; timing barely matters today.
    Flat,
}

impl Outlook {
    pub fn weakest(&self) -> Weakest {
        if self.best.coverage - self.worst.coverage < MIN_SPREAD {
            Weakest::Flat
        } else if self.worst.coverage >= GREEN_ANYWAY {
            Weakest::StillGreen
        } else {
            Weakest::Avoid
        }
    }
}

/// Midnight starting `day` in Germany, as UTC.
fn berlin_midnight(day: NaiveDate) -> DateTime<Utc> {
    Berlin
        .from_local_datetime(&day.and_time(NaiveTime::MIN))
        .earliest()
        .expect("German midnight always exists")
        .with_timezone(&Utc)
}

fn share(generation: f64, load: f64) -> f64 {
    if load > 0.0 {
        generation / load * 100.0
    } else {
        0.0
    }
}

/// Sum the points into whole hours. German offsets are whole hours, so UTC hour
/// buckets are also local hour buckets.
fn hourly(points: &[&RenewableSurplus]) -> Vec<Hour> {
    let mut hours: Vec<Hour> = Vec::new();
    for p in points {
        let start = p
            .timestamp
            .duration_trunc(Duration::hours(1))
            .unwrap_or(p.timestamp);
        match hours.last_mut() {
            Some(h) if h.start == start => {
                h.generation += p.generation;
                h.load += p.load;
            }
            _ => hours.push(Hour {
                start,
                generation: p.generation,
                load: p.load,
            }),
        }
    }
    hours
}

/// The `n`-hour run of consecutive hours with the highest (or lowest) coverage.
fn extreme_window(hours: &[Hour], n: usize, best: bool) -> Option<Window> {
    hours
        .windows(n)
        .filter(|w| w[n - 1].start - w[0].start == Duration::hours(n as i64 - 1))
        .map(|w| {
            let (g, l) = w
                .iter()
                .fold((0.0, 0.0), |(g, l), h| (g + h.generation, l + h.load));
            Window {
                start: w[0].start,
                end: w[n - 1].start + Duration::hours(1),
                coverage: share(g, l),
            }
        })
        .reduce(|a, b| {
            let b_wins = if best {
                b.coverage > a.coverage
            } else {
                b.coverage < a.coverage
            };
            if b_wins { b } else { a }
        })
}

/// Outlook for the rest of the German day containing `now` (from the current
/// hour to midnight). Late in the evening, when fewer than `MIN_HOURS` remain,
/// it covers the next day instead, if its forecast is already published.
pub fn outlook(series: &[RenewableSurplus], now: DateTime<Utc>) -> Option<Outlook> {
    let today = now.with_timezone(&Berlin).date_naive();
    let from = now.duration_trunc(Duration::hours(1)).ok()?;
    outlook_between(series, today, from).or_else(|| {
        let tomorrow = today.succ_opt()?;
        outlook_between(series, tomorrow, berlin_midnight(tomorrow))
    })
}

fn outlook_between(
    series: &[RenewableSurplus],
    day: NaiveDate,
    from: DateTime<Utc>,
) -> Option<Outlook> {
    let until = berlin_midnight(day.succ_opt()?);
    let pts: Vec<&RenewableSurplus> = series
        .iter()
        .filter(|p| p.timestamp >= from && p.timestamp < until)
        .collect();
    let hours = hourly(&pts);
    if hours.len() < MIN_HOURS {
        return None;
    }
    Some(Outlook {
        day,
        points: pts
            .iter()
            .map(|p| (p.timestamp, share(p.generation, p.load)))
            .collect(),
        peak: extreme_window(&hours, 1, true)?,
        best: extreme_window(&hours, WINDOW_HOURS, true)?,
        worst: extreme_window(&hours, WINDOW_HOURS, false)?,
    })
}

// ── Copy ─────────────────────────────────────────────────────────────────────

const WEEKDAYS: [&str; 7] = [
    "Montag",
    "Dienstag",
    "Mittwoch",
    "Donnerstag",
    "Freitag",
    "Samstag",
    "Sonntag",
];
const MONTHS: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// "Sonntag, 25. Oktober"
fn german_date(day: NaiveDate) -> String {
    format!(
        "{}, {}. {}",
        WEEKDAYS[day.weekday().num_days_from_monday() as usize],
        day.day(),
        MONTHS[day.month0() as usize]
    )
}

/// German local hour of `ts`; midnight at the end of a window reads as 24.
fn local_hour(ts: DateTime<Utc>, day: NaiveDate) -> u32 {
    let local = ts.with_timezone(&Berlin);
    if local.date_naive() > day {
        24
    } else {
        local.hour()
    }
}

/// "11–14 Uhr"
fn hour_range(w: &Window, day: NaiveDate) -> String {
    format!(
        "{}–{} Uhr",
        local_hour(w.start, day),
        local_hour(w.end, day)
    )
}

/// "87 %" — German typography puts a (narrow, non-breaking) space before %.
fn pct(v: f64) -> String {
    format!("{:.0}\u{202f}%", v)
}

/// Headline, the subline under it and the background tint, by how green the
/// best window is. Timing advice lives on the cards, not here.
fn verdict(o: &Outlook) -> (&'static str, String, &'static str) {
    let peak = pct(o.peak.coverage);
    let best = o.best.coverage;
    if best >= 100.0 {
        (
            "Ökostrom-Überschuss",
            format!("Wind & Sonne liefern bis zu {peak} des Bedarfs."),
            "#a7f3d0",
        )
    } else if best >= 60.0 {
        (
            "Viel Wind & Sonne",
            format!("Bis zu {peak} des Strombedarfs aus Wind & Sonne."),
            "#d1fae5",
        )
    } else if best >= 30.0 {
        (
            "Durchwachsener Tag",
            format!("Wind & Sonne decken bis zu {peak} des Bedarfs."),
            "#fef3c7",
        )
    } else {
        (
            "Wenig Wind & Sonne",
            format!("Höchstens {peak} des Bedarfs – heute Strom sparen."),
            "#e2e8f0",
        )
    }
}

/// XML-escape text for SVG.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ── SVG ──────────────────────────────────────────────────────────────────────

/// The educk duck (static/logo_bw.svg), in a 120×120 box.
const LOGO: &str = r##"<g transform="translate(-105.19661,-57.856903)"><circle cx="165.19661" cy="117.8569" r="60" fill="#0f172a"/><g transform="matrix(0.26458333,0,0,0.26458333,112.99601,81.961866)" fill="#ffffff" fill-rule="evenodd"><path d="m 145.14,121.86 c -9.648,-2.304 -18.144,-5.58 -26.316,-10.26 -8.208,-4.752 -15.948,-12.6 -21.96,-17.568 -5.868,-5.112 -9.036,-10.98 -13.68,-12.204 -4.86,-1.188 -9.576,-2.52 -15.12,4.896 -5.724,7.416 -13.896,25.416 -18.54,39.024 -4.824,13.356 -8.928,26.532 -9.252,41.472 -0.216,15.048 -0.108,33.479 7.812,47.808 7.956,14.112 24.012,28.296 39.024,36.576 14.904,8.208 26.028,9.828 49.752,12.204 23.76,2.088 66.349,1.764 91.224,0.972 24.588,-0.792 40.248,1.116 56.125,-5.363 15.84,-6.696 29.844,-21.708 38.555,-33.66 8.568,-12.061 12.744,-24.948 12.672,-37.548 -0.359,-12.708 -6.768,-27.36 -13.68,-37.584 -6.984,-10.261 -21.096,-16.165 -27.324,-23.4 -6.299,-7.344 -9.828,-12.168 -9.756,-20.016 0.037,-7.92 3.457,-16.668 10.26,-26.82 v -43.92 c -1.08,-2.016 -3.131,-4.68 -6.695,-8.28 -3.6,-3.672 -6.191,-8.712 -14.293,-13.176 -8.314,-4.572 -22.428,-11.628 -34.162,-13.68 -11.773,-2.124 -24.518,-1.908 -35.605,1.476 -11.268,3.312 -23.399,10.656 -30.708,18.54 -7.128,7.848 -10.548,19.368 -12.204,28.296 -1.872,8.712 -1.116,15.912 1.44,24.372 2.484,8.496 9.036,19.692 13.68,26.352 4.644,6.48 10.764,8.82 13.644,12.708 2.664,3.816 4.14,7.452 2.916,10.224 -1.332,2.628 -4.932,5.508 -10.224,5.868 -5.364,0.216 -15.12,-2.772 -21.348,-3.96 -6.3,-1.27 -11.67,-2.38 -16.24,-3.35 z"/><path d="m 289.36,53.78 c -0.576,-4.644 -2.951,-10.368 -2.951,-13.716 0,-3.42 0.973,-5.796 3.348,-6.3 2.34,-0.468 7.092,2.124 10.764,3.312 3.637,1.044 6.301,3.78 10.764,3.348 4.357,-0.432 11.557,-6.48 15.193,-5.94 3.527,0.432 5.652,3.924 5.939,8.532 0.145,4.464 2.053,11.448 -4.824,18.18 -7.02,6.552 -30.275,20.484 -36.719,21.528 -6.48,0.9 -1.693,-10.728 -1.873,-15.588 -0.29,-4.969 0.75,-8.821 0.36,-13.356 z"/></g></g>"##;

/// The coverage curve for the day: area + line, the 100 % line, and the best
/// (and, when timing matters, worst) window as labelled bands. Drawn into the
/// rectangle (x, y, w, h).
fn chart(o: &Outlook, x: f64, y: f64, w: f64, h: f64) -> String {
    let (pad_l, pad_b, pad_t) = (78.0, 54.0, 64.0);
    let (px, pw) = (x + pad_l, w - pad_l);
    let (py, ph) = (y + pad_t, h - pad_t - pad_b);

    let t0 = o.points[0]
        .0
        .duration_trunc(Duration::hours(1))
        .unwrap_or(o.points[0].0);
    let t1 = berlin_midnight(o.day.succ_opt().unwrap_or(o.day));
    let span = (t1 - t0).num_seconds().max(1) as f64;
    let max_c = o.points.iter().map(|p| p.1).fold(0.0_f64, f64::max);
    let max_y = (100.0_f64.max(max_c) * 1.2 / 25.0).ceil() * 25.0;
    let x_at = |t: DateTime<Utc>| px + (t - t0).num_seconds() as f64 / span * pw;
    let y_at = |v: f64| py + (1.0 - v / max_y) * ph;

    let mut s = String::new();

    // Bands behind everything, labelled so identity isn't color-alone.
    let mut band = |win: &Window, color: &str, label: &str| {
        let (bx0, bx1) = (x_at(win.start), x_at(win.end));
        s.push_str(&format!(
            "<rect x=\"{bx0:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"10\" fill=\"{color}\" fill-opacity=\"0.14\"/>\
             <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"26\" font-weight=\"600\" fill=\"{INK_2}\">{label}</text>",
            py - 44.0,
            bx1 - bx0,
            ph + 44.0,
            (bx0 + bx1) / 2.0,
            py - 14.0,
        ));
    };
    band(&o.best, GREEN, "Beste Zeit");
    if o.weakest() == Weakest::Avoid {
        band(&o.worst, AMBER, "Meiden");
    }

    // Recessive y grid every 50 %, 100 % ("all demand") emphasised.
    let mut v = 0.0;
    while v <= max_y + 0.1 {
        let gy = y_at(v);
        let (stroke, sw) = if (v - 100.0).abs() < 0.1 {
            ("#cbd5e1", 3.0)
        } else {
            (GRID, 2.0)
        };
        s.push_str(&format!(
            "<line x1=\"{px:.1}\" y1=\"{gy:.1}\" x2=\"{:.1}\" y2=\"{gy:.1}\" stroke=\"{stroke}\" stroke-width=\"{sw}\"/>\
             <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" font-size=\"26\" fill=\"{MUTED}\">{}</text>",
            px + pw,
            px - 14.0,
            gy + 9.0,
            pct(v),
        ));
        v += 50.0;
    }

    // Area + line.
    let line: Vec<String> = o
        .points
        .iter()
        .map(|&(t, c)| format!("{:.1},{:.1}", x_at(t), y_at(c)))
        .collect();
    let (first_x, last_x) = (x_at(o.points[0].0), x_at(o.points[o.points.len() - 1].0));
    let base = y_at(0.0);
    s.push_str(&format!(
        "<polygon points=\"{first_x:.1},{base:.1} {} {last_x:.1},{base:.1}\" fill=\"{GREEN}\" fill-opacity=\"0.12\"/>\
         <polyline points=\"{}\" fill=\"none\" stroke=\"{GREEN}\" stroke-width=\"6\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
        line.join(" "),
        line.join(" "),
    ));

    // Peak dot with a surface ring and its value (kept inside the plot).
    let peak_mid = o.peak.start + Duration::minutes(30);
    let (dx, dy) = (x_at(peak_mid).clamp(px, px + pw), y_at(o.peak.coverage));
    s.push_str(&format!(
        "<circle cx=\"{dx:.1}\" cy=\"{dy:.1}\" r=\"12\" fill=\"{GREEN}\" stroke=\"#ffffff\" stroke-width=\"5\"/>\
         <text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"30\" font-weight=\"700\" fill=\"{INK}\">{}</text>",
        dx.clamp(px + 48.0, px + pw - 48.0),
        dy - 26.0,
        pct(o.peak.coverage),
    ));

    // X labels every 3 local hours.
    let mut t = t0.duration_trunc(Duration::hours(1)).unwrap_or(t0);
    while t <= t1 {
        let hr = local_hour(t, o.day);
        // Ticks hugging the left edge would run into the "Uhr" unit label.
        if hr % 3 == 0 && x_at(t) - px > 30.0 {
            s.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"26\" fill=\"{MUTED}\">{hr}</text>",
                x_at(t),
                py + ph + 40.0,
            ));
        }
        t += Duration::hours(1);
    }
    // Unit for the hour ticks, under the y labels where nothing collides.
    s.push_str(&format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" font-size=\"26\" fill=\"{MUTED}\">Uhr</text>",
        px - 14.0,
        py + ph + 40.0,
    ));
    s
}

/// A rounded recommendation card at `y`: small caps label, big text, one line
/// of advice. A colored side bar carries the status next to the label text.
/// Returns the SVG and the card's height.
fn card(y: f64, color: &str, label: &str, big: &str, big_size: f64, note: &str) -> (String, f64) {
    let label_y = y + 64.0;
    let big_y = label_y + 22.0 + big_size * 0.74;
    let note_y = big_y + 58.0;
    let h = note_y - y + 42.0;
    let svg = format!(
        "<rect x=\"72\" y=\"{y:.0}\" width=\"936\" height=\"{h:.0}\" rx=\"32\" fill=\"#ffffff\"/>\
         <rect x=\"72\" y=\"{y:.0}\" width=\"14\" height=\"{h:.0}\" rx=\"7\" fill=\"{color}\"/>\
         <text x=\"124\" y=\"{label_y:.0}\" font-size=\"28\" font-weight=\"600\" letter-spacing=\"3\" fill=\"{INK_2}\">{}</text>\
         <text x=\"120\" y=\"{big_y:.0}\" font-size=\"{big_size:.0}\" font-weight=\"700\" letter-spacing=\"-2\" fill=\"{INK}\">{}</text>\
         <text x=\"124\" y=\"{note_y:.0}\" font-size=\"34\" fill=\"{INK_2}\">{}</text>",
        esc(&label.to_uppercase()),
        esc(big),
        esc(note),
    );
    (svg, h)
}

/// The full story as an SVG document. Instagram overlays its own UI on roughly
/// the top and bottom 220 px, so the content sits between those.
pub fn svg(o: &Outlook) -> String {
    let (headline, subline, tint) = verdict(o);
    let (best, worst) = (&o.best, &o.worst);

    let (best_card, best_h) = card(
        1056.0,
        GREEN,
        "Beste Zeit",
        &hour_range(best, o.day),
        104.0,
        &format!("Waschen, Spülen, E-Auto laden · Ø {}", pct(best.coverage)),
    );
    let (color, label, big, note) = match o.weakest() {
        Weakest::Avoid => (
            AMBER,
            "Lieber meiden",
            hour_range(worst, o.day),
            format!("Nur Ø {} aus Wind & Sonne", pct(worst.coverage)),
        ),
        Weakest::StillGreen => (
            "#34d399",
            "Schwächste Zeit",
            hour_range(worst, o.day),
            format!("Immer noch Ø {} aus Wind & Sonne", pct(worst.coverage)),
        ),
        Weakest::Flat => (
            MUTED,
            "Gleichmäßiger Tag",
            "Kaum Unterschied".to_string(),
            format!(
                "Ganztags Ø {:.0}–{} aus Wind & Sonne",
                worst.coverage,
                pct(best.coverage)
            ),
        ),
    };
    let (second_card, second_h) = card(1056.0 + best_h + 24.0, color, label, &big, 64.0, &note);
    let footer_y = 1056.0 + best_h + 24.0 + second_h + 40.0;

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" viewBox=\"0 0 {WIDTH} {HEIGHT}\" font-family=\"Inter\">\
         <defs><linearGradient id=\"bg\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"{tint}\"/><stop offset=\"0.8\" stop-color=\"{tint}\" stop-opacity=\"0\"/>\
         </linearGradient></defs>\
         <rect width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"#f1f5f9\"/>\
         <rect width=\"{WIDTH}\" height=\"{HEIGHT}\" fill=\"url(#bg)\"/>\
         <text x=\"80\" y=\"290\" font-size=\"32\" font-weight=\"600\" letter-spacing=\"3\" fill=\"{INK_2}\">{date}</text>\
         <text x=\"76\" y=\"378\" font-size=\"80\" font-weight=\"700\" letter-spacing=\"-2.5\" fill=\"{INK}\">{headline}</text>\
         <text x=\"80\" y=\"440\" font-size=\"36\" fill=\"{INK_2}\">{subline}</text>\
         <rect x=\"72\" y=\"496\" width=\"936\" height=\"530\" rx=\"32\" fill=\"#ffffff\"/>\
         <text x=\"112\" y=\"560\" font-size=\"30\" font-weight=\"600\" fill=\"{INK}\">Anteil Wind &amp; Sonne am Strombedarf</text>\
         {chart}\
         {best_card}{second_card}\
         <svg x=\"80\" y=\"{footer_y:.0}\" width=\"56\" height=\"56\" viewBox=\"0 0 120 120\">{LOGO}</svg>\
         <text x=\"152\" y=\"{:.0}\" font-size=\"30\" font-weight=\"700\" fill=\"{INK}\">educk.io</text>\
         <text x=\"152\" y=\"{:.0}\" font-size=\"22\" fill=\"{MUTED}\">Prognose Wind &amp; Solar: ENTSO-E · Deutschland</text>\
         </svg>",
        footer_y + 30.0,
        footer_y + 62.0,
        date = esc(&german_date(o.day).to_uppercase()),
        headline = esc(headline),
        subline = esc(&subline),
        chart = chart(o, 112.0, 580.0, 856.0, 420.0),
    )
}

/// Render the story as a JPEG.
pub fn jpeg(o: &Outlook) -> anyhow::Result<Vec<u8>> {
    let opt = usvg::Options {
        fontdb: FONTS.clone(),
        font_family: "Inter".to_string(),
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(&svg(o), &opt)?;
    let mut pixmap =
        tiny_skia::Pixmap::new(WIDTH, HEIGHT).ok_or_else(|| anyhow::anyhow!("pixmap"))?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    // The background is opaque, so premultiplied RGBA is plain RGBA here.
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, JPEG_QUALITY).encode(
        pixmap.data(),
        WIDTH as u16,
        HEIGHT as u16,
        jpeg_encoder::ColorType::Rgba,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// 15-min points across the German `day`, with load fixed at 100 MW so
    /// `coverage(local_hour)` is the generation in MW (= coverage %).
    fn series(day: NaiveDate, coverage: impl Fn(f64) -> f64) -> Vec<RenewableSurplus> {
        let (start, end) = (
            berlin_midnight(day),
            berlin_midnight(day.succ_opt().unwrap()),
        );
        let mut out = Vec::new();
        let mut t = start;
        while t < end {
            let local = t.with_timezone(&Berlin);
            let generation = coverage(local.hour() as f64 + local.minute() as f64 / 60.0);
            out.push(RenewableSurplus {
                timestamp: t,
                generation,
                load: 100.0,
                surplus: generation - 100.0,
            });
            t += Duration::minutes(15);
        }
        out
    }

    /// A sunny, calm day: solar bell around 13:00, little wind.
    fn solar_day(h: f64) -> f64 {
        15.0 + 70.0 * (-((h - 13.0) / 3.0).powi(2)).exp()
    }

    #[test]
    fn finds_best_and_worst_windows_in_local_time() {
        // A summer day (CEST, UTC+2): times must come out in German local time.
        let d = day(2026, 6, 15);
        let now = berlin_midnight(d) + Duration::hours(7); // 07:00 CEST
        let o = outlook(&series(d, solar_day), now).unwrap();
        assert_eq!(o.day, d);
        assert_eq!(hour_range(&o.best, d), "12–15 Uhr");
        assert_eq!(local_hour(o.peak.start, d), 13);
        assert_eq!(hour_range(&o.worst, d), "21–24 Uhr");
        assert_eq!(o.weakest(), Weakest::Avoid);
        assert_eq!(verdict(&o).0, "Viel Wind & Sonne");
    }

    #[test]
    fn only_recommends_avoiding_a_clearly_worse_window() {
        let d = day(2026, 1, 12);
        let flat = outlook(&series(d, |h| 40.0 + h * 0.2), berlin_midnight(d)).unwrap();
        assert_eq!(flat.weakest(), Weakest::Flat);
        // Windy all day: the dip is real but still mostly renewable.
        let windy = outlook(&series(d, |h| 120.0 - h), berlin_midnight(d)).unwrap();
        assert_eq!(windy.weakest(), Weakest::StillGreen);
    }

    #[test]
    fn handles_the_25_hour_dst_day() {
        // 2026-10-25: clocks go back, the German day has 25 hours.
        let d = day(2026, 10, 25);
        assert_eq!(
            berlin_midnight(d.succ_opt().unwrap()) - berlin_midnight(d),
            Duration::hours(25)
        );
        let o = outlook(&series(d, solar_day), berlin_midnight(d)).unwrap();
        assert_eq!(hour_range(&o.best, d), "12–15 Uhr");
    }

    #[test]
    fn late_evening_covers_the_next_day() {
        let d = day(2026, 3, 9);
        let mut s = series(d, solar_day);
        s.extend(series(d.succ_opt().unwrap(), solar_day));
        let now = berlin_midnight(d) + Duration::hours(21); // 21:00 CET, 3h left
        assert_eq!(outlook(&s, now).unwrap().day, d.succ_opt().unwrap());
        // Without tomorrow's forecast there is nothing worth posting.
        assert!(outlook(&series(d, solar_day), now).is_none());
    }

    #[test]
    fn formats_german_dates() {
        assert_eq!(german_date(day(2026, 10, 25)), "Sonntag, 25. Oktober");
        assert_eq!(german_date(day(2026, 3, 2)), "Montag, 2. März");
    }

    #[test]
    fn renders_a_story_sized_jpeg() {
        let d = day(2026, 6, 15);
        let o = outlook(&series(d, solar_day), berlin_midnight(d)).unwrap();
        let jpg = jpeg(&o).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        // Baseline SOF0 segment: FF C0, len(2), precision(1), height(2), width(2).
        let sof = jpg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        let dim = |i: usize| u16::from_be_bytes([jpg[sof + i], jpg[sof + i + 1]]) as u32;
        assert_eq!((dim(5), dim(7)), (HEIGHT, WIDTH));
    }
}
