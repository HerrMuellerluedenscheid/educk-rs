//! Internationalisation for the server-rendered site.
//!
//! The SSR pages are bilingual (English + German). Language is carried in a
//! `?lang=de` query parameter (English lives at the bare paths). This module owns:
//!   - the [`Lang`] enum and `Accept-Language` negotiation,
//!   - the [`Strings`] table of static UI chrome (one `const` per language),
//!   - localized prose builders for the parameterized copy (titles, leads, …),
//!   - URL helpers that keep the language across internal links.
//!
//! Times shown to users stay in UTC; only the surrounding prose is translated.

/// Site UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    /// BCP-47 / `<html lang>` code.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// Parse an explicit `?lang=` value. Unknown values yield `None`.
    pub fn from_query(value: Option<&str>) -> Option<Lang> {
        match value {
            Some("de") => Some(Lang::De),
            Some("en") => Some(Lang::En),
            _ => None,
        }
    }
}

/// Pick the visitor's preferred language from an `Accept-Language` header.
///
/// Returns `De` only when the highest-priority language tag is German; anything
/// else (including a missing/garbled header) falls back to English. Crawlers send
/// `en`/nothing, so they always get the English (canonical) variant.
pub fn preferred_from_accept_language(header: &str) -> Lang {
    let mut best: Option<(f32, &str)> = None;
    for part in header.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.split(';');
        let tag = it.next().unwrap_or("").trim();
        // q-value (default 1.0); ignore malformed.
        let q = it
            .find_map(|p| p.trim().strip_prefix("q="))
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(1.0);
        match best {
            // Strictly greater so that, on a tie, the earlier (higher-priority)
            // tag wins — matching header ordering semantics.
            Some((bq, _)) if q <= bq => {}
            _ => best = Some((q, tag)),
        }
    }
    match best {
        Some((_, tag)) if tag.to_ascii_lowercase().starts_with("de") => Lang::De,
        _ => Lang::En,
    }
}

/// Append (or replace) a `lang` query parameter on a path that may already carry a
/// query string, e.g. `/?country=FR` + `de` -> `/?country=FR&lang=de`.
pub fn add_lang_param(path_and_query: &str, lang_code: &str) -> String {
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => return format!("{path_and_query}?lang={lang_code}"),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| !kv.is_empty() && !kv.starts_with("lang="))
        .collect();
    if kept.is_empty() {
        format!("{path}?lang={lang_code}")
    } else {
        format!("{path}?{}&lang={lang_code}", kept.join("&"))
    }
}

/// Localize an internal link: German links carry `?lang=de`; English links are
/// left at the bare path (the canonical English URL).
pub fn localize_url(path: &str, lang: Lang) -> String {
    match lang {
        Lang::En => path.to_string(),
        Lang::De => add_lang_param(path, "de"),
    }
}

/// Absolute canonical + `hreflang` alternate URLs for a page at `path` (the bare,
/// language-neutral path, e.g. `/electricity/de`). The canonical is self-referencing
/// for the current language; the English alternate is the bare URL (also used as
/// `x-default`) and the German alternate carries `?lang=de`.
pub fn page_urls(base_url: &str, path: &str, lang: Lang) -> (String, String, String) {
    let alt_en = format!("{base_url}{path}");
    let alt_de = format!("{base_url}{}", add_lang_param(path, "de"));
    let canonical = match lang {
        Lang::En => alt_en.clone(),
        Lang::De => alt_de.clone(),
    };
    (canonical, alt_en, alt_de)
}

/// German display name for a country, keyed by its English `BiddingZone::name`.
/// Returns `None` for names we don't translate (caller falls back to English).
pub fn de_country_name(en_name: &str) -> Option<&'static str> {
    Some(match en_name {
        "Albania" => "Albanien",
        "Austria" => "Österreich",
        "Belarus" => "Belarus",
        "Belgium" => "Belgien",
        "Bosnia and Herzegovina" => "Bosnien und Herzegowina",
        "Bulgaria" => "Bulgarien",
        "Croatia" => "Kroatien",
        "Cyprus" => "Zypern",
        "Czech Republic" => "Tschechien",
        "Denmark" => "Dänemark",
        "Estonia" => "Estland",
        "Finland" => "Finnland",
        "France" => "Frankreich",
        "Germany" => "Deutschland",
        "Greece" => "Griechenland",
        "Hungary" => "Ungarn",
        "Iceland" => "Island",
        "Ireland" => "Irland",
        "Italy" => "Italien",
        "Kaliningrad" => "Kaliningrad",
        "Latvia" => "Lettland",
        "Lithuania" => "Litauen",
        "Luxembourg" => "Luxemburg",
        "North Macedonia" => "Nordmazedonien",
        "Malta" => "Malta",
        "Moldova" => "Moldau",
        "Montenegro" => "Montenegro",
        "Netherlands" => "Niederlande",
        "Norway" => "Norwegen",
        "Poland" => "Polen",
        "Portugal" => "Portugal",
        "Romania" => "Rumänien",
        "Russia" => "Russland",
        "Serbia" => "Serbien",
        "Slovakia" => "Slowakei",
        "Slovenia" => "Slowenien",
        "Spain" => "Spanien",
        "Sweden" => "Schweden",
        "Switzerland" => "Schweiz",
        "Turkey" => "Türkei",
        "Ukraine" => "Ukraine",
        _ => return None,
    })
}

/// Localized country display name (falls back to the English name).
pub fn country_name(en_name: &str, lang: Lang) -> String {
    match lang {
        Lang::En => en_name.to_string(),
        Lang::De => de_country_name(en_name).unwrap_or(en_name).to_string(),
    }
}

/// Static UI chrome strings referenced directly from the templates as `t.*`.
/// Fields documented as "(HTML)" contain markup and are rendered with `|safe`.
pub struct Strings {
    pub code: &'static str,
    // nav
    pub nav_all_countries: &'static str,
    pub nav_all_regions: &'static str,
    pub nav_impressum: &'static str,
    pub nav_privacy: &'static str,
    pub back_to_educk: &'static str,
    // landing
    pub index_h1: &'static str,
    pub index_lead: &'static str,
    pub cta_dashboard: &'static str,
    pub picker_label: &'static str,
    pub show: &'static str,
    pub label_forecast_for: &'static str,
    pub label_updated: &'static str,
    pub show_table: &'static str,
    pub how_h2: &'static str,
    pub how_1: &'static str, // (HTML)
    pub how_2: &'static str,
    pub how_3: &'static str,
    pub by_country_h2: &'static str,
    pub by_country_intro: &'static str,
    pub by_cloud_h2: &'static str,
    pub by_cloud_intro: &'static str,
    // dashboard (SSR first-paint snapshot; hydrated by static/app/app.js)
    pub gauge_renewable_now: &'static str,
    pub card_surplus_now: &'static str,
    pub card_deficit_now: &'static str,
    pub card_of_generation: &'static str,
    pub card_peak_surplus: &'static str,
    pub card_green_coverage: &'static str,
    pub card_of_current_load: &'static str,
    pub trend_rising: &'static str,
    pub trend_falling: &'static str,
    pub trend_steady: &'static str,
    pub chart_now: &'static str,
    pub chart_data_points: &'static str,
    pub hint_good: &'static str, // contains the "{time}" placeholder
    pub hint_bad: &'static str,  // contains the "{time}" placeholder
    // shared table headers
    pub th_time: &'static str,
    pub th_renewable: &'static str,
    pub th_demand: &'static str,
    pub th_surplus: &'static str,
    pub th_share: &'static str,
    // country / cloud labels
    pub label_renewable_forecast: &'static str,
    pub label_carbon_aware: &'static str,
    // footer
    pub footer_blurb: &'static str, // (HTML)
    // cookie consent
    pub cookie_text: &'static str, // (HTML)
    pub cookie_decline: &'static str,
    pub cookie_accept: &'static str,
}

pub const EN: Strings = Strings {
    code: "en",
    nav_all_countries: "All countries",
    nav_all_regions: "All regions",
    nav_impressum: "Impressum",
    nav_privacy: "Privacy",
    back_to_educk: "← Back to educk",
    index_h1: "When is electricity greenest across Europe?",
    index_lead: "educk is a renewable energy monitor. It tracks how much of each \
        European country's electricity demand is met by wind and solar generation — \
        live and as a day-ahead forecast — so you can shift flexible energy use to \
        the cleanest, lowest-carbon hours of the day.",
    cta_dashboard: "Go to Dashboard →",
    picker_label: "Show forecast for",
    show: "Show",
    label_forecast_for: "Forecast for",
    label_updated: "updated",
    show_table: "Show hourly data table",
    how_h2: "How it works",
    how_1: "We pull day-ahead wind &amp; solar generation and total demand forecasts \
        from the <a href=\"https://transparency.entsoe.eu/\" rel=\"nofollow noopener\">ENTSO-E</a> \
        transparency platform.",
    how_2: "For each country we work out the renewable share of demand hour by hour, \
        and when it peaks.",
    how_3: "You move flexible loads — EV charging, laundry, heating, batteries, \
        compute — into the greenest windows.",
    by_country_h2: "Renewable electricity by country",
    by_country_intro: "Pick a country to see its renewable energy forecast and the \
        greenest time to use electricity today:",
    by_cloud_h2: "Renewable electricity by cloud region",
    by_cloud_intro: "Scheduling compute? See when each European cloud region's grid \
        is greenest and run carbon-aware workloads then:",
    gauge_renewable_now: "renewable now",
    card_surplus_now: "Surplus now",
    card_deficit_now: "Deficit now",
    card_of_generation: "of generation",
    card_peak_surplus: "Peak surplus",
    card_green_coverage: "Green coverage",
    card_of_current_load: "of current load",
    trend_rising: "Rising",
    trend_falling: "Falling",
    trend_steady: "Steady",
    chart_now: "Now",
    chart_data_points: "data points",
    hint_good: "Best time to charge an EV or run high-energy appliances: {time} — \
        that's when renewables exceed demand by the most.",
    hint_bad: "Renewables don't cover full demand in this window. Highest renewable \
        share is around {time}.",
    th_time: "Time (UTC)",
    th_renewable: "Renewable (MW)",
    th_demand: "Demand (MW)",
    th_surplus: "Surplus (MW)",
    th_share: "Share",
    label_renewable_forecast: "Renewable energy forecast",
    label_carbon_aware: "Carbon-aware scheduling",
    footer_blurb: "<strong>educk</strong> shows real-time and forecast renewable \
        electricity share across European bidding zones so you can shift consumption \
        to greener, lower-carbon hours. Data source: \
        <a href=\"https://transparency.entsoe.eu/\" rel=\"nofollow noopener\">ENTSO-E</a>.",
    cookie_text: "educk uses Google Analytics to understand how the site is used. \
        Analytics cookies are only set if you agree. See our \
        <a href=\"/privacy\">privacy&nbsp;policy</a> for details — you can change your \
        choice anytime by clearing the <code>educk_consent</code> cookie.",
    cookie_decline: "Decline",
    cookie_accept: "Accept",
};

pub const DE: Strings = Strings {
    code: "de",
    nav_all_countries: "Alle Länder",
    nav_all_regions: "Alle Regionen",
    nav_impressum: "Impressum",
    nav_privacy: "Datenschutz",
    back_to_educk: "← Zurück zu educk",
    index_h1: "Wann ist Strom in Europa am grünsten?",
    index_lead: "educk ist ein Monitor für erneuerbare Energien. Er zeigt, wie viel \
        des Strombedarfs jedes europäischen Landes durch Wind- und Solarenergie \
        gedeckt wird — live und als Day-Ahead-Prognose — damit Sie flexiblen \
        Stromverbrauch in die saubersten, CO₂-ärmsten Stunden des Tages verlegen \
        können.",
    cta_dashboard: "Zum Dashboard →",
    picker_label: "Prognose anzeigen für",
    show: "Anzeigen",
    label_forecast_for: "Prognose für",
    label_updated: "aktualisiert",
    show_table: "Stündliche Datentabelle anzeigen",
    how_h2: "So funktioniert's",
    how_1: "Wir beziehen Day-Ahead-Prognosen für Wind- &amp; Solarerzeugung und \
        Gesamtlast von der <a href=\"https://transparency.entsoe.eu/\" rel=\"nofollow noopener\">ENTSO-E</a>-Transparenzplattform.",
    how_2: "Für jedes Land berechnen wir den Erneuerbaren-Anteil am Bedarf Stunde \
        für Stunde — und wann er am höchsten ist.",
    how_3: "Sie verlagern flexible Lasten — E-Auto-Laden, Wäsche, Heizen, Batterien, \
        Rechenlast — in die grünsten Zeitfenster.",
    by_country_h2: "Erneuerbarer Strom nach Land",
    by_country_intro: "Wählen Sie ein Land, um seine Erneuerbaren-Prognose und die \
        grünste Zeit für den Stromverbrauch heute zu sehen:",
    by_cloud_h2: "Erneuerbarer Strom nach Cloud-Region",
    by_cloud_intro: "Sie planen Rechenlast? Sehen Sie, wann das Netz jeder \
        europäischen Cloud-Region am grünsten ist, und führen Sie CO₂-bewusste \
        Workloads dann aus:",
    gauge_renewable_now: "erneuerbar jetzt",
    card_surplus_now: "Überschuss jetzt",
    card_deficit_now: "Defizit jetzt",
    card_of_generation: "der Erzeugung",
    card_peak_surplus: "Spitzenüberschuss",
    card_green_coverage: "Grüne Deckung",
    card_of_current_load: "des aktuellen Bedarfs",
    trend_rising: "Steigend",
    trend_falling: "Fallend",
    trend_steady: "Stabil",
    chart_now: "Jetzt",
    chart_data_points: "Datenpunkte",
    hint_good: "Beste Zeit zum Laden eines E-Autos oder Betreiben energieintensiver \
        Geräte: {time} — dann übersteigen die Erneuerbaren den Bedarf am stärksten.",
    hint_bad: "Die Erneuerbaren decken in diesem Zeitfenster nicht den gesamten \
        Bedarf. Der höchste Erneuerbaren-Anteil liegt gegen {time}.",
    th_time: "Zeit (UTC)",
    th_renewable: "Erneuerbar (MW)",
    th_demand: "Bedarf (MW)",
    th_surplus: "Überschuss (MW)",
    th_share: "Anteil",
    label_renewable_forecast: "Erneuerbare-Energien-Prognose",
    label_carbon_aware: "CO₂-bewusste Planung",
    footer_blurb: "<strong>educk</strong> zeigt den Echtzeit- und Prognose-Anteil \
        erneuerbaren Stroms in europäischen Gebotszonen, damit Sie Ihren Verbrauch \
        in grünere, CO₂-ärmere Stunden verlegen können. Datenquelle: \
        <a href=\"https://transparency.entsoe.eu/\" rel=\"nofollow noopener\">ENTSO-E</a>.",
    cookie_text: "educk nutzt Google Analytics, um zu verstehen, wie die Seite \
        genutzt wird. Analyse-Cookies werden nur mit Ihrer Zustimmung gesetzt. \
        Details in unserer <a href=\"/privacy?lang=de\">Datenschutzerklärung</a> — \
        Sie können Ihre Wahl jederzeit ändern, indem Sie das Cookie \
        <code>educk_consent</code> löschen.",
    cookie_decline: "Ablehnen",
    cookie_accept: "Akzeptieren",
};

/// The static string table for a language.
pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::En => &EN,
        Lang::De => &DE,
    }
}

// ── Parameterized prose builders ─────────────────────────────────────────────
// These interpolate runtime values (country names, regions) so they're built in
// the handler and passed to the template as ready strings.

pub fn index_title(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "educk — when is electricity greenest across Europe?",
        Lang::De => "educk — wann ist Strom in Europa am grünsten?",
    }
}

pub fn index_meta(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "educk shows live and day-ahead renewable electricity share \
            across European grids, so you can shift energy use to cleaner, \
            lower-carbon hours.",
        Lang::De => "educk zeigt den Live- und Day-Ahead-Anteil erneuerbaren Stroms \
            in europäischen Netzen, damit Sie Ihren Verbrauch in sauberere, \
            CO₂-ärmere Stunden verlegen.",
    }
}

pub fn country_title(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!(
            "When is electricity greenest in {country}? Renewable energy forecast | educk"
        ),
        Lang::De => {
            format!("Wann ist Strom in {country} am grünsten? Erneuerbare-Energien-Prognose | educk")
        }
    }
}

pub fn country_h1(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!("When is electricity greenest in {country}?"),
        Lang::De => format!("Wann ist Strom in {country} am grünsten?"),
    }
}

pub fn country_lead(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!(
            "educk tracks how much of {country}'s electricity demand is met by wind \
             and solar generation, using day-ahead forecasts from the ENTSO-E \
             transparency platform. Use it to plan flexible energy use — charging an \
             EV, running appliances or batteries — for when the grid is cleanest."
        ),
        Lang::De => format!(
            "educk zeigt, wie viel des Strombedarfs von {country} durch Wind- und \
             Solarenergie gedeckt wird — anhand von Day-Ahead-Prognosen der \
             ENTSO-E-Transparenzplattform. Planen Sie damit flexiblen Verbrauch — \
             E-Auto laden, Geräte oder Batterien betreiben — für die Zeiten, in denen \
             das Netz am saubersten ist."
        ),
    }
}

pub fn country_hourly_h2(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!("Hourly forecast for {country}"),
        Lang::De => format!("Stündliche Prognose für {country}"),
    }
}

/// JSON-LD `about.name` for a country page.
pub fn country_about(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!("Renewable electricity in {country}"),
        Lang::De => format!("Erneuerbarer Strom in {country}"),
    }
}

pub fn cloud_title(lang: Lang, provider: &str, region: &str, location: &str) -> String {
    match lang {
        Lang::En => {
            format!("Greenest time to run workloads in {provider}/{region} ({location}) | educk")
        }
        Lang::De => format!("Grünste Zeit für Workloads in {provider}/{region} ({location}) | educk"),
    }
}

pub fn cloud_h1(lang: Lang, provider_label: &str, region: &str) -> String {
    match lang {
        Lang::En => format!("When is {provider_label} {region} greenest?"),
        Lang::De => format!("Wann ist {provider_label} {region} am grünsten?"),
    }
}

/// Cloud lead paragraph (contains `<code>` markup — render with `|safe`).
pub fn cloud_lead(
    lang: Lang,
    provider_label: &str,
    region: &str,
    location: &str,
    country: &str,
) -> String {
    match lang {
        Lang::En => format!(
            "The {provider_label} <code>{region}</code> region runs in {location}, on \
             the {country} electricity grid. educk forecasts when that grid is \
             greenest — using day-ahead wind &amp; solar and demand data from ENTSO-E \
             — so you can schedule carbon-aware workloads (batch jobs, CI, model \
             training, backups, data pipelines) for the cleanest hours."
        ),
        Lang::De => format!(
            "Die {provider_label}-Region <code>{region}</code> läuft in {location}, im \
             Stromnetz von {country}. educk prognostiziert, wann dieses Netz am \
             grünsten ist — anhand von Day-Ahead-Daten zu Wind &amp; Solar und Bedarf \
             von ENTSO-E — damit Sie CO₂-bewusste Workloads (Batch-Jobs, CI, \
             Modelltraining, Backups, Daten-Pipelines) in die saubersten Stunden \
             legen können."
        ),
    }
}

pub fn cloud_hourly_h2(lang: Lang, provider_label: &str, region: &str) -> String {
    match lang {
        Lang::En => format!("Hourly grid forecast for {provider_label} {region}"),
        Lang::De => format!("Stündliche Netzprognose für {provider_label} {region}"),
    }
}

pub fn cloud_cta(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!("Full {country} forecast"),
        Lang::De => format!("Vollständige Prognose für {country}"),
    }
}

/// JSON-LD `about.name` for a cloud page.
pub fn cloud_about(lang: Lang, provider_label: &str, region: &str) -> String {
    match lang {
        Lang::En => format!("Carbon-aware scheduling for {provider_label} {region}"),
        Lang::De => format!("CO₂-bewusste Planung für {provider_label} {region}"),
    }
}

/// Landing table caption (includes the selected country name).
pub fn caption_landing(lang: Lang, subject: &str) -> String {
    match lang {
        Lang::En => format!(
            "Wind + solar generation vs. total demand (MW) — {subject}, next hours · times in UTC"
        ),
        Lang::De => format!(
            "Wind- + Solarerzeugung vs. Gesamtbedarf (MW) — {subject}, nächste Stunden · Zeiten in UTC"
        ),
    }
}

/// Country page table caption (no subject in the string).
pub fn caption_country(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Wind + solar generation vs. total demand (MW), next hours · times in UTC",
        Lang::De => "Wind- + Solarerzeugung vs. Gesamtbedarf (MW), nächste Stunden · Zeiten in UTC",
    }
}

/// Cloud page table caption (includes the underlying national grid name).
pub fn caption_cloud(lang: Lang, country: &str) -> String {
    match lang {
        Lang::En => format!(
            "Wind + solar generation vs. total demand (MW) on the {country} grid, next hours · times in UTC"
        ),
        Lang::De => format!(
            "Wind- + Solarerzeugung vs. Gesamtbedarf (MW) im Netz von {country}, nächste Stunden · Zeiten in UTC"
        ),
    }
}

/// JSON-LD `ItemList` name on the landing page.
pub fn itemlist_name(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "Renewable electricity forecast by country",
        Lang::De => "Erneuerbarer-Strom-Prognose nach Land",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language_prefers_german_when_top() {
        assert_eq!(
            preferred_from_accept_language("de-DE,de;q=0.9,en;q=0.8"),
            Lang::De
        );
        assert_eq!(preferred_from_accept_language("de"), Lang::De);
    }

    #[test]
    fn accept_language_defaults_english() {
        assert_eq!(preferred_from_accept_language("en-US,en;q=0.9"), Lang::En);
        assert_eq!(preferred_from_accept_language(""), Lang::En);
        // English higher q wins even when listed second.
        assert_eq!(
            preferred_from_accept_language("de;q=0.5,en-US;q=0.9"),
            Lang::En
        );
    }

    #[test]
    fn add_lang_param_handles_existing_query() {
        assert_eq!(add_lang_param("/", "de"), "/?lang=de");
        assert_eq!(add_lang_param("/electricity/de", "de"), "/electricity/de?lang=de");
        assert_eq!(add_lang_param("/?country=FR", "de"), "/?country=FR&lang=de");
        // An existing lang is replaced, not duplicated.
        assert_eq!(add_lang_param("/?lang=en", "de"), "/?lang=de");
    }

    #[test]
    fn localize_url_only_tags_german() {
        assert_eq!(localize_url("/electricity/de", Lang::En), "/electricity/de");
        assert_eq!(
            localize_url("/electricity/de", Lang::De),
            "/electricity/de?lang=de"
        );
    }
}
