# Build stage
FROM rust:1.90 as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

COPY . .
# Build the actual application
RUN cargo build --release

# Runtime stage
FROM debian:trixie-slim

# Install CA certificates for HTTPS requests
RUN apt-get update && \
    apt-get install -y ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/educk-rs /app/educk-rs
COPY --from=builder /app/templates /app/templates

# Expose port
EXPOSE 3044

# Set environment variable
ENV RUST_LOG=info

# Run the application
CMD ["/app/educk-rs"]