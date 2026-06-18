# =============================================================================
# Stage 1: Builder
# Uses the official Rust image to compile a fully static release binary.
# =============================================================================
FROM rust:1.82-slim AS builder

# Install system dependencies required by reqwest (OpenSSL/TLS support)
RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first to leverage Docker layer caching for dependencies.
# If only source code changes, the expensive `cargo build` of dependencies
# is cached from the previous image build.
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs so cargo can resolve and build all dependencies.
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Now copy the actual source code and build the real binary.
COPY src/ src/
RUN cargo build --release

# =============================================================================
# Stage 2: Runtime
# Minimal image containing only the compiled binary and TLS certificates.
# =============================================================================
FROM debian:bookworm-slim

# Install CA certificates for HTTPS and libssl for TLS runtime support.
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage.
COPY --from=builder /app/target/release/imagen /usr/local/bin/imagen

# Document all configuration environment variables with empty defaults.
# These must be set at runtime (via docker run -e or docker-compose).
ENV IMAGEN_PROVIDER=""
ENV OPENAI_API_KEY=""
ENV OPENAI_ORG_ID=""
ENV AZURE_OPENAI_ENDPOINT=""
ENV AZURE_OPENAI_DEPLOYMENT=""
ENV AZURE_OPENAI_API_KEY=""
ENV AZURE_OPENAI_API_VERSION=""
ENV IMAGEN_OUTPUT_DIR="/data/output"
ENV IMAGEN_MAX_CONCURRENT_JOBS="4"
ENV IMAGEN_DEFAULT_MODEL="gpt-image-2"

# Create a non-root user for security best practices.
RUN useradd -r -u 1000 -m imagen

# Create the default output directory with correct ownership.
RUN mkdir -p /data/output && chown imagen:imagen /data/output

# Run as non-root user.
USER imagen

# The imagen binary communicates over stdio (MCP transport).
ENTRYPOINT ["imagen"]
