# educk-rs — task runner
# Requires: just (https://github.com/casey/just)

# ── Development ───────────────────────────────────────────────────────────────

# Run the backend + Flutter frontend together (Ctrl-C stops both)
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    [ -f flutter/.env ] || cp flutter/.env.example flutter/.env
    cargo run &
    (cd flutter && flutter run -d chrome --dart-define-from-file=.env) &
    wait

# ── Rust backend ─────────────────────────────────────────────────────────────

# Run the API server (requires ENTSOE_API_KEY in environment)
api:
    cargo run

# Build a release binary
build-api:
    cargo build --release

# Run backend tests
test-api:
    cargo test

# ── Flutter frontend ──────────────────────────────────────────────────────────

# Install Flutter dependencies
deps:
    cd flutter && flutter pub get

# Run the Flutter app (development)
run-flutter:
    #!/usr/bin/env bash
    set -euo pipefail
    cd flutter
    [ -f .env ] || cp .env.example .env
    flutter run -d chrome --dart-define-from-file=.env

# Build Flutter web for deployment (output: flutter/build/web)
build-web:
    cd flutter && flutter build web --release

# ── Docker ────────────────────────────────────────────────────────────────────

# Build and start all services (requires .env with ENTSOE_API_KEY)
up:
    docker compose up --build -d

# Stop all services
down:
    docker compose down

# Stream logs from all services
logs:
    docker compose logs -f

# Rebuild and restart a single service: just restart api | dashboard
restart service:
    docker compose up --build -d {{service}}
