#
# Praxis Docker Image
# Multi-stage build for minimal runtime image.
#

# ==============================================================================
# Stage 1: Build the Rust binaries and frontend
# ==============================================================================
FROM rust:1.88-bookworm AS builder

RUN apt-get update && apt-get install -y \
    nodejs \
    npm \
    pkg-config \
    libssl-dev \
    mingw-w64 \
    && rm -rf /var/lib/apt/lists/*

#
# Add Windows cross-compilation target.
#
RUN rustup target add x86_64-pc-windows-gnu

#
# Configure cargo for Windows cross-compilation.
#
RUN mkdir -p /root/.cargo && echo '\
[target.x86_64-pc-windows-gnu]\n\
linker = "x86_64-w64-mingw32-gcc"\n\
' >> /root/.cargo/config.toml

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY common ./common
COPY node ./node
COPY semantic_parser ./semantic_parser
COPY semantic_ops ./semantic_ops
COPY service ./service
COPY web ./web

#
# Build praxis_node for Linux and Windows first (before frontend, as requested).
#
RUN cargo build --release -p praxis_node && \
    cargo build --release -p praxis_node --target x86_64-pc-windows-gnu

#
# Build frontend (npm install + build).
#
WORKDIR /build/web/frontend
RUN npm ci && npm run build

#
# Build service and web binaries.
#
WORKDIR /build
RUN cargo build --release -p praxis_service -p praxis_web

# ==============================================================================
# Stage 2: Runtime image
# ==============================================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    netcat-openbsd \
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
