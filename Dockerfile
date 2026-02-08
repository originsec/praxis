#
# Praxis Docker Image
# Multi-stage build for minimal runtime image with dependency caching.
#

# ==============================================================================
# Stage 1: Prepare recipe for cargo-chef
# ==============================================================================
FROM rust:1.88-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /build

# ==============================================================================
# Stage 2: Analyze dependencies
# ==============================================================================
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY agents ./agents
COPY cli ./cli
COPY common ./common
COPY node ./node
COPY semantic_parser ./semantic_parser
COPY service ./service
COPY web ./web
RUN cargo chef prepare --recipe-path recipe.json

# ==============================================================================
# Stage 3: Build frontend with Node 22
# ==============================================================================
FROM node:22-bookworm-slim AS frontend

WORKDIR /build/web/frontend
COPY web/frontend/package*.json ./
RUN npm ci
COPY web/frontend ./
RUN npm run build

# ==============================================================================
# Stage 4: Build Rust dependencies (cached layer)
# ==============================================================================
FROM chef AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    mingw-w64 \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-gnu

RUN mkdir -p /root/.cargo && echo '\
[target.x86_64-pc-windows-gnu]\n\
linker = "x86_64-w64-mingw32-gcc"\n\
' >> /root/.cargo/config.toml

WORKDIR /build

#
# Build dependencies only - this layer is cached until Cargo.toml/Cargo.lock changes.
#

COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p praxis_node && \
    cargo chef cook --release --recipe-path recipe.json -p praxis_node --target x86_64-pc-windows-gnu && \
    cargo chef cook --release --recipe-path recipe.json -p praxis_service -p praxis_web

# ==============================================================================
# Stage 5: Build application (only recompiles on source changes)
# ==============================================================================
COPY Cargo.toml Cargo.lock ./
COPY agents ./agents
COPY cli ./cli
COPY common ./common
COPY node ./node
COPY semantic_parser ./semantic_parser
COPY service ./service
COPY web ./web

#
# Copy pre-built frontend from frontend stage.
#

COPY --from=frontend /build/web/frontend/dist ./web/frontend/dist

#
# Skip frontend build in build.rs since it's already built above.
#

ENV PRAXIS_SKIP_FRONTEND=1

#
# Build praxis_node for Linux and Windows.
#

RUN cargo build --release -p praxis_node && \
    cargo build --release -p praxis_node --target x86_64-pc-windows-gnu

#
# Build service and web binaries.
#

RUN cargo build --release -p praxis_service -p praxis_web

# ==============================================================================
# Stage 6: Runtime image
# ==============================================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    netcat-openbsd \
    iptables \
    iproute2 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

#
# Copy main binaries.
#

COPY --from=builder /build/target/release/praxis_service /app/
COPY --from=builder /build/target/release/praxis_web /app/

#
# Copy node binaries for download.
#

RUN mkdir -p /app/nodes
COPY --from=builder /build/target/release/praxis_node /app/nodes/praxis_node_linux
COPY --from=builder /build/target/x86_64-pc-windows-gnu/release/praxis_node.exe /app/nodes/praxis_node_windows.exe

#
# Copy and setup entrypoint script.
#

COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

ENV PRAXIS_RABBITMQ_URL=amqp://praxis:praxis@rabbitmq:5672
ENV PRAXIS_NODES_DIR=/app/nodes

EXPOSE 8080

ENTRYPOINT ["/app/entrypoint.sh"]
