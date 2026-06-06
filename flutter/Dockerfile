# ── Stage 1: build Flutter web ─────────────────────────────────────────────────
FROM ghcr.io/cirruslabs/flutter:stable AS builder

WORKDIR /app

# Copy pubspec first so dependency layer is cached separately from source
COPY pubspec.yaml ./
RUN flutter pub get

# Copy rest of the source
COPY . .

# Scaffold the web/ directory if it doesn't exist yet
# (safe to run on existing projects — won't overwrite our Dart files)
RUN flutter create --project-name energy_dashboard --platforms web . \
    > /dev/null 2>&1 || true

# Bake the API URL and git commit into the JS bundle at compile time.
# GIT_COMMIT is supplied by the build invoker (.git is not in this context).
ARG API_URL=https://api.educk.io
ARG GIT_COMMIT=unknown
# Served under /app at the apex (nginx proxies the rest to the Rust backend), so
# the bundle must be built with a matching base href.
RUN flutter build web --release \
    --base-href=/app/ \
    --dart-define=API_URL=${API_URL} \
    --dart-define=GIT_COMMIT=${GIT_COMMIT}


# ── Stage 2: serve with nginx ───────────────────────────────────────────────────
FROM nginx:alpine-slim

# Bundle lives under app/ so nginx can serve it at /app/ and proxy the rest.
COPY --from=builder /app/build/web /usr/share/nginx/html/app
COPY nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80

# Health-check the statically-served app entry point (not /, which proxies to the
# Rust backend) so the dashboard's health doesn't depend on the API being up.
# Use 127.0.0.1 rather than localhost: in this image `localhost` resolves to
# IPv6 ::1 first, where nginx (IPv4 0.0.0.0:80 only) isn't listening, so the
# check would fail and the container would never report healthy.
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1/app/ || exit 1
