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

# Bake the API URL into the JS bundle at compile time
ARG API_URL=http://37.27.47.184:3044
RUN flutter build web --release --dart-define=API_URL=${API_URL}


# ── Stage 2: serve with nginx ───────────────────────────────────────────────────
FROM nginx:alpine-slim

COPY --from=builder /app/build/web /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD wget -qO- http://localhost/ || exit 1
