# Multi-stage Dockerfile for gh0st web crawler
# Produces a minimal, secure container image

# Build stage
FROM rust:1.75-slim as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
  pkg-config \
  libssl-dev \
  musl-tools \
  && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Create a dummy main to cache dependencies
RUN mkdir src && \
  echo "fn main() {}" > src/main.rs && \
  cargo build --release && \
  rm -rf src

# Copy source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
  ca-certificates \
  curl \
  && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -u 1000 gh0st

# Copy the binary from builder
COPY --from=builder /app/target/release/gh0st /usr/local/bin/gh0st

# Set ownership
RUN chown gh0st:gh0st /usr/local/bin/gh0st

# Create directory for output files
RUN mkdir -p /data && chown gh0st:gh0st /data

# Switch to non-root user
USER gh0st

# Set working directory
WORKDIR /data

# Default command
ENTRYPOINT ["gh0st"]
CMD ["--help"]

# Labels
LABEL org.opencontainers.image.title="gh0st"
LABEL org.opencontainers.image.description="A powerful TUI web crawler and SEO analyzer"
LABEL org.opencontainers.image.authors="Michael Wallner"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.source="https://github.com/yourusername/gh0st"
