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


# ── Stage 2: precompress static assets ───────────────────────────────────────────
# Build-time brotli (-q 11) and gzip (-9) so nginx can serve the precompressed
# variants via brotli_static/gzip_static instead of compressing on every request.
# Worth the max compression level here: the bundle is immutable (content-hashed),
# so each file is compressed once at build and served many times.
FROM alpine AS compress
RUN apk add --no-cache brotli gzip
COPY --from=builder /app/build/web /web
RUN find /web -type f \( \
        -name '*.js'   -o -name '*.css'  -o -name '*.wasm' -o -name '*.json' \
        -o -name '*.svg' -o -name '*.html' -o -name '*.ttf'  -o -name '*.otf' \
    \) -print0 \
    | while IFS= read -r -d '' f; do \
        brotli -q 11 -f "$f"; \
        gzip  -9 -k -f "$f"; \
      done


# ── Stage 3: build the brotli dynamic modules for nginx ──────────────────────────
# The official nginx image has no brotli (it's a third-party module). We compile
# ngx_brotli against the *exact* nginx version from the runtime image, using
# --with-compat so the resulting .so is ABI-compatible with the precompiled
# nginx binary (the official image is itself built with --with-compat).
FROM nginx:alpine-slim AS brotli-build
RUN apk add --no-cache \
    git gcc make libc-dev linux-headers \
    pcre2-dev zlib-dev openssl-dev brotli-dev
WORKDIR /build
RUN set -eux; \
    NGINX_VERSION="$(nginx -V 2>&1 | sed -n 's@.*nginx/\([0-9.]*\).*@\1@p')"; \
    wget -O nginx.tar.gz "https://nginx.org/download/nginx-${NGINX_VERSION}.tar.gz"; \
    tar -xzf nginx.tar.gz; \
    git clone --depth=1 --recurse-submodules https://github.com/google/ngx_brotli.git; \
    cd "nginx-${NGINX_VERSION}"; \
    ./configure --with-compat --add-dynamic-module=../ngx_brotli; \
    make -j"$(nproc)" modules; \
    mkdir -p /modules; \
    cp objs/ngx_http_brotli_filter_module.so \
       objs/ngx_http_brotli_static_module.so /modules/


# ── Stage 4: serve with nginx ───────────────────────────────────────────────────
FROM nginx:alpine-slim

# brotli-libs provides libbrotlienc/libbrotlidec that the dynamic modules link against.
RUN apk add --no-cache brotli-libs

# Brotli dynamic modules + the load_module directives (which must live in the main
# context, not in conf.d/*.conf which is included inside http{}).
COPY --from=brotli-build /modules/ /etc/nginx/modules/
RUN sed -i '1i load_module modules/ngx_http_brotli_filter_module.so;\nload_module modules/ngx_http_brotli_static_module.so;' \
    /etc/nginx/nginx.conf

# Bundle (with precompressed .br/.gz siblings) lives under app/ so nginx can serve
# it at /app/ and proxy the rest.
COPY --from=compress /web /usr/share/nginx/html/app
COPY nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80

# Health-check the statically-served app entry point (not /, which proxies to the
# Rust backend) so the dashboard's health doesn't depend on the API being up.
# Use 127.0.0.1 rather than localhost: in this image `localhost` resolves to
# IPv6 ::1 first, where nginx (IPv4 0.0.0.0:80 only) isn't listening, so the
# check would fail and the container would never report healthy.
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1/app/ || exit 1
