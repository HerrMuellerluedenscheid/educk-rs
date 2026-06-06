# Educk

Renewable energy surplus dashboard. A Rust API fetches generation and load forecasts from [ENTSO-E](https://transparency.entsoe.eu/) and a Flutter web app visualises them.

```
educk-rs/
├── src/        Rust API server (Axum, port 3044)
├── flutter/    Flutter dashboard (web + mobile)
└── templates/  Plotly HTML templates
```

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

- Dashboard: http://localhost
- API: http://localhost:3044

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

The `docker-compose.yaml` builds a multi-stage image (Flutter → Nginx) and
wires it to the API container. Pass a custom API URL at build time:

```sh
API_URL=https://api.example.com docker compose up --build -d
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `ENTSOE_API_KEY` | — | ENTSO-E transparency platform key |
| `API_URL` | `https://api.educk.io` | URL the browser uses to reach the API |
| `RUST_LOG` | `info` | Rust log level |
