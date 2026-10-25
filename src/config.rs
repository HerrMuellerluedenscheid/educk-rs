use anyhow::Context;
use serde::Deserialize;

/// Application configuration, loaded from a `.env` file merged with the process
/// environment.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub entsoe_api_key: String,

    /// Public origin the SSR content pages live under, used for canonical URLs,
    /// Open Graph tags and the sitemap. Override with `PUBLIC_BASE_URL`.
    #[serde(default = "default_public_base_url")]
    pub public_base_url: String,

    /// Long-lived Instagram User access token (Instagram API with Instagram
    /// Login, `instagram_business_content_publish`). When set, the daily Story
    /// for Germany is published; unset or empty disables it.
    #[serde(default)]
    pub instagram_access_token: Option<String>,

    /// German wall-clock time ("HH:MM") the daily Story goes out.
    #[serde(default = "default_instagram_story_time")]
    pub instagram_story_time: String,

    /// Directory for small persistent state (the auto-refreshed Instagram token,
    /// the date of the last story). Mount it as a volume in Docker.
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_public_base_url() -> String {
    "https://educk.io".to_string()
}

fn default_instagram_story_time() -> String {
    "07:00".to_string()
}

fn default_data_dir() -> String {
    "data".to_string()
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // dotenvy does not override already-set vars, so external env (Docker, CI) wins over .env.
        dotenvy::dotenv().ok();

        config::Config::builder()
            .add_source(config::Environment::default())
            .build()?
            .try_deserialize()
            .context("invalid configuration; is ENTSOE_API_KEY set?")
    }
}
