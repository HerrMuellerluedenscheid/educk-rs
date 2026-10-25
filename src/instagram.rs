//! Publishes the daily Story (see `story`) through the Instagram API with
//! Instagram Login: create a STORIES container from the public image URL, wait
//! until Meta has fetched it, publish. Runs as a background task, once a day at
//! a fixed German wall-clock time.
//!
//! The long-lived access token expires after 60 days, so it is refreshed weekly.
//! The current token and the date of the last story live in a small JSON file,
//! which survives restarts: a deploy never posts twice, nor loses the token.

use std::future::Future;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const HOST: &str = "https://graph.instagram.com";
const API_VERSION: &str = "v26.0";
/// Long-lived tokens last 60 days; refresh well before that.
const REFRESH_EVERY: Duration = Duration::days(7);
/// After a missed slot (server down, upstream failing) keep trying this long.
const CATCH_UP: Duration = Duration::hours(3);
/// Pause between attempts when posting fails.
const RETRY: std::time::Duration = std::time::Duration::from_secs(600);
/// Container status polls (Meta suggests ≤ 1/min for ≤ 5 min; images are
/// usually ready within seconds, so poll faster at first).
const STATUS_POLLS: u32 = 10;

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// The configured token this state was seeded from. A different one in the
    /// environment (e.g. after re-authorising) replaces the stored token.
    seed_token: String,
    access_token: String,
    /// When the current token was issued (seeded or last refreshed).
    token_since: Option<DateTime<Utc>>,
    /// German date of the last published story.
    last_story: Option<NaiveDate>,
}

pub struct Publisher {
    http: Client,
    state_path: PathBuf,
    state: State,
    post_at: NaiveTime,
    /// Instagram professional account ID, looked up from the token.
    user_id: Option<String>,
}

impl Publisher {
    pub fn new(access_token: String, data_dir: &str, post_at: NaiveTime) -> anyhow::Result<Self> {
        let state_path = Path::new(data_dir).join("instagram.json");
        let mut state: State = match std::fs::read_to_string(&state_path) {
            Ok(json) => serde_json::from_str(&json).with_context(|| {
                format!("corrupt Instagram state file {}", state_path.display())
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e).context(format!("reading {}", state_path.display())),
        };
        if state.seed_token != access_token {
            state.seed_token = access_token.clone();
            state.access_token = access_token;
            state.token_since = Some(Utc::now());
        }
        let publisher = Self {
            http: Client::new(),
            state_path,
            state,
            post_at,
            user_id: None,
        };
        publisher.save()?;
        Ok(publisher)
    }

    /// Write the state atomically, readable only by us (it holds the token).
    fn save(&self) -> anyhow::Result<()> {
        use std::io::Write;
        if let Some(dir) = self.state_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.state_path.with_extension("json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        opts.open(&tmp)?
            .write_all(serde_json::to_string_pretty(&self.state)?.as_bytes())?;
        std::fs::rename(&tmp, &self.state_path)
            .with_context(|| format!("writing {}", self.state_path.display()))
    }

    /// Call the Graph API, turning its `{"error": {...}}` bodies into errors.
    /// The token travels as a query parameter, so request errors are stripped of
    /// their URL before they can end up in a log line.
    async fn call(
        &self,
        method: Method,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Value> {
        let mut url = Url::parse(&format!("{HOST}/{path}"))?;
        url.query_pairs_mut()
            .extend_pairs(params)
            .append_pair("access_token", &self.state.access_token);
        let resp = self
            .http
            .request(method, url)
            .send()
            .await
            .map_err(|e| e.without_url())?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| e.without_url())?;
        let json: Value = serde_json::from_str(&body).with_context(|| {
            format!("Instagram API returned HTTP {status} with a non-JSON body")
        })?;
        if let Some(err) = json.get("error") {
            bail!(
                "Instagram API error {} (HTTP {status}): {}",
                err["code"],
                err["message"].as_str().unwrap_or("no message")
            );
        }
        if !status.is_success() {
            bail!("Instagram API returned HTTP {status}");
        }
        Ok(json)
    }

    /// The professional account ID behind the token (cached after the first call).
    async fn user_id(&mut self) -> anyhow::Result<String> {
        if let Some(id) = &self.user_id {
            return Ok(id.clone());
        }
        let me = self
            .call(
                Method::GET,
                &format!("{API_VERSION}/me"),
                &[("fields", "user_id,username")],
            )
            .await?;
        let id = match &me["user_id"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => bail!("Instagram /me returned no user_id"),
        };
        tracing::info!(
            "Instagram: publishing stories as @{}",
            me["username"].as_str().unwrap_or("?")
        );
        self.user_id = Some(id.clone());
        Ok(id)
    }

    async fn refresh_token_if_due(&mut self) -> anyhow::Result<()> {
        if self
            .state
            .token_since
            .is_some_and(|t| Utc::now() - t < REFRESH_EVERY)
        {
            return Ok(());
        }
        let resp = self
            .call(
                Method::GET,
                "refresh_access_token",
                &[("grant_type", "ig_refresh_token")],
            )
            .await?;
        self.state.access_token = string_field(&resp, "access_token")?;
        self.state.token_since = Some(Utc::now());
        self.save()?;
        tracing::info!(
            "Instagram: access token refreshed, valid for {} days",
            resp["expires_in"].as_i64().unwrap_or(0) / 86_400
        );
        Ok(())
    }

    /// Publish the image at `image_url` (a public JPEG) as a story. Returns the
    /// media ID.
    pub async fn publish_story(&mut self, image_url: &str) -> anyhow::Result<String> {
        let user = self.user_id().await?;
        let container = self
            .call(
                Method::POST,
                &format!("{API_VERSION}/{user}/media"),
                &[("media_type", "STORIES"), ("image_url", image_url)],
            )
            .await?;
        let container_id = string_field(&container, "id")?;

        // Meta downloads the image asynchronously; publish once it's processed.
        for poll in 0..STATUS_POLLS {
            let wait = if poll == 0 { 5 } else { 30 };
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            let status = self
                .call(
                    Method::GET,
                    &format!("{API_VERSION}/{container_id}"),
                    &[("fields", "status_code")],
                )
                .await?;
            match status["status_code"].as_str() {
                Some("FINISHED") => break,
                Some("IN_PROGRESS") if poll + 1 < STATUS_POLLS => continue,
                other => bail!("story container {container_id} not publishable: {other:?}"),
            }
        }

        let media = self
            .call(
                Method::POST,
                &format!("{API_VERSION}/{user}/media_publish"),
                &[("creation_id", container_id.as_str())],
            )
            .await?;
        string_field(&media, "id")
    }

    /// Post a story every day at `post_at` (German time). `prepare(day)` makes
    /// sure the story for `day` renders and returns its public image URL.
    pub async fn run<F, Fut>(mut self, prepare: F)
    where
        F: Fn(NaiveDate) -> Fut,
        Fut: Future<Output = anyhow::Result<String>>,
    {
        // Check the token up front so a bad one shows in the deploy logs, not at 07:00.
        if let Err(e) = self.user_id().await {
            tracing::error!("Instagram: token check failed: {e:#}");
        }
        loop {
            match next_step(Utc::now(), self.post_at, self.state.last_story) {
                Step::SleepUntil(at) => {
                    tracing::info!(
                        "Instagram: next story at {}",
                        at.with_timezone(&Berlin).format("%Y-%m-%d %H:%M %Z")
                    );
                    tokio::time::sleep((at - Utc::now()).to_std().unwrap_or_default()).await;
                }
                Step::Post(day) => {
                    if let Err(e) = self.refresh_token_if_due().await {
                        tracing::warn!("Instagram: token refresh failed: {e:#}");
                    }
                    let result = match prepare(day).await {
                        Ok(url) => self.publish_story(&url).await,
                        Err(e) => Err(e),
                    };
                    match result {
                        Ok(media_id) => {
                            tracing::info!("Instagram: published story {media_id} for {day}");
                            self.state.last_story = Some(day);
                            if let Err(e) = self.save() {
                                tracing::error!("Instagram: could not save state: {e:#}");
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Instagram: story for {day} failed, retrying in 10 min: {e:#}"
                            );
                            tokio::time::sleep(RETRY).await;
                        }
                    }
                }
            }
        }
    }
}

fn string_field(json: &Value, key: &str) -> anyhow::Result<String> {
    json[key]
        .as_str()
        .map(str::to_string)
        .with_context(|| format!("Instagram API response has no {key:?}"))
}

#[derive(Debug, PartialEq)]
enum Step {
    Post(NaiveDate),
    SleepUntil(DateTime<Utc>),
}

/// `at` on the German calendar `day`, as UTC. A time inside the spring-forward
/// gap moves an hour later.
fn slot(day: NaiveDate, at: NaiveTime) -> DateTime<Utc> {
    let local = day.and_time(at);
    Berlin
        .from_local_datetime(&local)
        .earliest()
        .or_else(|| {
            Berlin
                .from_local_datetime(&(local + Duration::hours(1)))
                .earliest()
        })
        .expect("a German wall-clock time exists at most an hour later")
        .with_timezone(&Utc)
}

/// What the daily loop does at `now`: post today's story if its slot has come
/// (and is less than `CATCH_UP` old) and it isn't out yet, else sleep until the
/// next slot.
fn next_step(now: DateTime<Utc>, post_at: NaiveTime, last_story: Option<NaiveDate>) -> Step {
    let today = now.with_timezone(&Berlin).date_naive();
    let today_slot = slot(today, post_at);
    if last_story == Some(today) || now >= today_slot + CATCH_UP {
        Step::SleepUntil(slot(today.succ_opt().unwrap_or(today), post_at))
    } else if now < today_slot {
        Step::SleepUntil(today_slot)
    } else {
        Step::Post(today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn berlin(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Berlin
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    fn seven() -> NaiveTime {
        NaiveTime::from_hms_opt(7, 0, 0).unwrap()
    }

    #[test]
    fn posts_once_per_day_in_german_time() {
        let today = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        // Before the slot: wait for 07:00 CEST (05:00 UTC).
        assert_eq!(
            next_step(berlin(2026, 6, 15, 6, 0), seven(), None),
            Step::SleepUntil(berlin(2026, 6, 15, 7, 0))
        );
        // At or shortly after the slot: post.
        assert_eq!(
            next_step(berlin(2026, 6, 15, 7, 0), seven(), None),
            Step::Post(today)
        );
        assert_eq!(
            next_step(berlin(2026, 6, 15, 9, 30), seven(), None),
            Step::Post(today)
        );
        // Already posted, or the slot is long gone: tomorrow.
        assert_eq!(
            next_step(berlin(2026, 6, 15, 7, 5), seven(), Some(today)),
            Step::SleepUntil(berlin(2026, 6, 16, 7, 0))
        );
        assert_eq!(
            next_step(berlin(2026, 6, 15, 10, 0), seven(), None),
            Step::SleepUntil(berlin(2026, 6, 16, 7, 0))
        );
    }

    #[test]
    fn slot_follows_daylight_saving_changes() {
        // 07:00 is 05:00 UTC in summer and 06:00 UTC in winter, either side of
        // the 2026-10-25 change.
        let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        assert_eq!(
            slot(day(24), seven()),
            Utc.with_ymd_and_hms(2026, 10, 24, 5, 0, 0).unwrap()
        );
        assert_eq!(
            slot(day(25), seven()),
            Utc.with_ymd_and_hms(2026, 10, 25, 6, 0, 0).unwrap()
        );
        // A time skipped by spring-forward (02:30 on 2026-03-29) moves to 03:30.
        let gap = slot(
            NaiveDate::from_ymd_opt(2026, 3, 29).unwrap(),
            NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
        );
        assert_eq!(gap, Utc.with_ymd_and_hms(2026, 3, 29, 1, 30, 0).unwrap());
    }

    #[test]
    fn a_new_configured_token_replaces_the_stored_one() {
        let dir = std::env::temp_dir().join(format!("educk-ig-test-{}", std::process::id()));
        let data_dir = dir.to_str().unwrap();
        let mut p = Publisher::new("first".into(), data_dir, seven()).unwrap();
        // Simulate a refresh and a published story, then a restart.
        p.state.access_token = "refreshed".into();
        p.state.last_story = NaiveDate::from_ymd_opt(2026, 6, 15);
        p.save().unwrap();
        let p = Publisher::new("first".into(), data_dir, seven()).unwrap();
        assert_eq!(p.state.access_token, "refreshed");
        // Re-authorised with a new token: it wins, the posting history stays.
        let p = Publisher::new("second".into(), data_dir, seven()).unwrap();
        assert_eq!(p.state.access_token, "second");
        assert_eq!(p.state.last_story, NaiveDate::from_ymd_opt(2026, 6, 15));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
