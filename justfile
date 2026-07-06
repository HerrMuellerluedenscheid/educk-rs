# educk-rs — task runner
# Requires: just (https://github.com/casey/just)

# ── Development ───────────────────────────────────────────────────────────────

# Run the backend + Flutter frontend together (Ctrl-C stops both)
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    [ -f flutter/.env ] || cp flutter/.env.example flutter/.env
    cargo run --bin educk-rs &
    (cd flutter && flutter run --dart-define-from-file=.env) &
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

# ── Dashboard (lightweight /app: vanilla HTML/JS/SVG + Tailwind) ────────────────
# The dashboard lives in static/app/ and is embedded into the Rust binary, served
# at /app. Run it with `just api` and open http://localhost:3044/app.

# Tailwind standalone version + platform (override tw_platform on non-mac-arm64,
# e.g. `just tw_platform=linux-x64 build-css`).
tw_version := "v4.3.1"
tw_platform := "macos-arm64"

# Regenerate the committed dashboard CSS (static/app/app.css). Downloads the
# Tailwind standalone CLI into bin/ on first run — no Node toolchain needed.
build-css:
    #!/usr/bin/env bash
    set -euo pipefail
    bin="bin/tailwindcss"
    if [ ! -x "$bin" ]; then
      mkdir -p bin
      url="https://github.com/tailwindlabs/tailwindcss/releases/download/{{tw_version}}/tailwindcss-{{tw_platform}}"
      echo "downloading tailwindcss {{tw_version}}"
      curl -sSL -o "$bin" "$url"
      chmod +x "$bin"
    fi
    "$bin" -i static/app/input.css -o static/app/app.css --minify
    echo "wrote static/app/app.css"

# Rebuild the dashboard CSS on change while editing static/app/
watch-css:
    bin/tailwindcss -i static/app/input.css -o static/app/app.css --watch

# ── Flutter frontend (mobile; web dashboard now lives in the Rust /app) ─────────

# Install Flutter dependencies
deps:
    cd flutter && flutter pub get

# start ios simulator
start-simulator:
    open -a Simulator

# Run the Flutter app (development)
run-flutter:
    #!/usr/bin/env bash
    set -euo pipefail
    cd flutter
    [ -f .env ] || cp .env.example .env
    flutter run --dart-define-from-file=.env

# Build Flutter web for deployment (output: flutter/build/web)
build-web:
    cd flutter && flutter build web --release

# ── Docker ────────────────────────────────────────────────────────────────────

# Build and start all services (requires .env with ENTSOE_API_KEY)
up:
    GIT_COMMIT=$(git rev-parse --short HEAD) docker compose up --build -d

# Stop all services
down:
    docker compose down

# Stream logs from all services
logs:
    docker compose logs -f

# Rebuild and restart a single service: just restart renewable-api
restart service:
    GIT_COMMIT=$(git rev-parse --short HEAD) docker compose up --build -d {{service}}
