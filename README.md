# Educk

Renewable energy surplus dashboard. A Rust API fetches generation and load forecasts from [ENTSO-E](https://transparency.entsoe.eu/) and a Flutter web app visualises them.

```
educk-rs/
├── src/        Rust API server (Axum, port 3044) — also serves the SSR content pages
├── flutter/    Flutter dashboard (web + mobile)
└── templates/  Askama / Plotly HTML templates
```

## Routing

At the apex (`educk.io`) nginx is the front door:

| Path | Served by | Notes |
|---|---|---|
| `/` , `/electricity/<country>` | Rust (SSR) | Crawlable content pages — the SEO surface |
| `/robots.txt` , `/sitemap.xml` | Rust | Generated; sitemap lists every country page |
| `/app/` | nginx (static) | The Flutter dashboard (built with `--base-href=/app/`) |
| `api.educk.io` → `/api/v1/*` | Rust | JSON API |

Flutter web renders to canvas and is invisible to crawlers, so the Rust-rendered
pages — not the app — are what search engines and link-preview bots index.

## Prerequisites

- [Rust](https://rustup.rs/)
- [Flutter SDK](https://docs.flutter.dev/get-started/install) (with web support enabled)
- [Docker](https://docs.docker.com/get-docker/) + Docker Compose
- [just](https://github.com/casey/just) task runner

An **ENTSO-E API key** is required — request one at https://transparency.entsoe.eu/

## Quick start

```sh
cp .env.example .env          # add ENTSOE_API_KEY to .env
just up                        # build images and start both services
```

- Content site (apex): http://localhost — e.g. http://localhost/electricity/be
- Dashboard: http://localhost/app/
- API (direct): http://localhost:3044

## Development

```sh
# Backend
just api                       # run API server (uses $ENTSOE_API_KEY)
just test-api

# Frontend
just deps                      # flutter pub get
just run-flutter               # run with flutter run
```

## Deployment

Build the Flutter web app and publish it (e.g. via Docker):

```sh
just build-web                 # compiles to flutter/build/web
just up                        # or ship the docker-compose stack
```

The `docker-compose.yaml` builds a multi-stage image (Flutter → Nginx) that
serves the dashboard at `/app/` and proxies everything else to the Rust API
container. Pass a custom API URL at build time:

```sh
API_URL=https://api.example.com docker compose up --build -d
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `ENTSOE_API_KEY` | — | ENTSO-E transparency platform key |
| `API_URL` | `https://api.educk.io` | URL the browser uses to reach the API (baked into the Flutter bundle at build time) |
| `INSTAGRAM_ACCESS_TOKEN` | — | Enables the daily Instagram Story (see below) |
| `INSTAGRAM_STORY_TIME` | `07:00` | German wall-clock time the Story is published |
| `DATA_DIR` | `data` | Persistent state (refreshed Instagram token, last-posted date); a volume in Docker |

## Daily Instagram Story

Every morning the server publishes a German-language Story for Germany: the share
of demand that wind and solar cover through the day, the best 3-hour window to run
flexible loads (laundry, dishwasher, EV) and the window to avoid. Preview the
current one at <https://educk.io/social/story/de.jpg>. Instagram fetches the image
from that URL when publishing, so it must be publicly reachable.

One-time setup (Instagram API with Instagram Login):

1. Switch the Instagram account to a professional account (Business or Creator).
2. At <https://developers.facebook.com/apps> create an app with the Instagram use
   case, open *API setup with Instagram business login*, add the account and
   generate an access token with `instagram_business_basic` and
   `instagram_business_content_publish`.
3. Put it in the server's `.env` as `INSTAGRAM_ACCESS_TOKEN=…` and `just up`. The
   logs confirm the account (`publishing stories as @…`) and the next slot.

The token lasts 60 days; the server refreshes it weekly and keeps the current one
in `$DATA_DIR/instagram.json`. If it ever expires anyway, generate a new one and
replace `INSTAGRAM_ACCESS_TOKEN`: a changed token replaces the stored one. A
missed slot (deploy, ENTSO-E outage) is retried for up to three hours, and the
last-posted date in the state file prevents double posts.
| `PUBLIC_BASE_URL` | `https://educk.io` | Apex origin for canonical URLs, Open Graph tags and the sitemap |
| `RUST_LOG` | `info` | Rust log level |
