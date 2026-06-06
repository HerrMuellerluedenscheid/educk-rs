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
}

fn default_public_base_url() -> String {
    "https://educk.io".to_string()
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
