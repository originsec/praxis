#
# Praxis Docker Image
# Multi-stage build for a systemd-based runtime.
#
# The runtime stage uses systemd as PID 1 so `praxisctl` works the
# same way inside the container as it does on a native Linux install.
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
ARG SKIP_NODE_BUILD=0
ARG SKIP_WEB_BUILD=0
ARG CARGO_PROFILE=release

RUN apt-get update && apt-get install -y pkg-config libssl-dev \
    && if [ "$SKIP_NODE_BUILD" = "0" ]; then apt-get install -y mingw-w64; fi \
    && rm -rf /var/lib/apt/lists/*

RUN if [ "$SKIP_NODE_BUILD" = "0" ]; then \
    rustup target add x86_64-pc-windows-gnu && \
    mkdir -p /root/.cargo && echo '\
[target.x86_64-pc-windows-gnu]\n\
linker = "x86_64-w64-mingw32-gcc"\n\
' >> /root/.cargo/config.toml; \
    fi

WORKDIR /build

#
# Build dependencies only - this layer is cached until Cargo.toml/Cargo.lock changes.
#

COPY --from=planner /build/recipe.json recipe.json
RUN if [ "$SKIP_NODE_BUILD" = "0" ]; then \
        cargo chef cook --profile "$CARGO_PROFILE" --recipe-path recipe.json -p praxis_node && \
        cargo chef cook --profile "$CARGO_PROFILE" --recipe-path recipe.json -p praxis_node --target x86_64-pc-windows-gnu; \
    fi && \
    if [ "$SKIP_WEB_BUILD" = "1" ]; then \
        cargo chef cook --profile "$CARGO_PROFILE" --recipe-path recipe.json -p praxis_service -p praxis_cli; \
    else \
        cargo chef cook --profile "$CARGO_PROFILE" --recipe-path recipe.json -p praxis_service -p praxis_web -p praxis_cli; \
    fi

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

ENV PRAXIS_SKIP_FRONTEND=1

RUN if [ "$SKIP_NODE_BUILD" = "0" ]; then \
        cargo build --profile "$CARGO_PROFILE" -p praxis_node && \
        cargo build --profile "$CARGO_PROFILE" -p praxis_node --target x86_64-pc-windows-gnu; \
    else \
        mkdir -p "target/$CARGO_PROFILE" "target/x86_64-pc-windows-gnu/$CARGO_PROFILE" && \
        touch "target/$CARGO_PROFILE/praxis_node" "target/x86_64-pc-windows-gnu/$CARGO_PROFILE/praxis_node.exe"; \
    fi

RUN if [ "$SKIP_WEB_BUILD" = "1" ]; then \
        cargo build --profile "$CARGO_PROFILE" -p praxis_service -p praxis_cli && \
        touch "target/$CARGO_PROFILE/praxis_web"; \
    else \
        cargo build --profile "$CARGO_PROFILE" -p praxis_service -p praxis_web -p praxis_cli; \
    fi

# ==============================================================================
# Stage 6: Runtime image (systemd as PID 1)
# ==============================================================================
FROM debian:bookworm-slim
ARG CARGO_PROFILE=release

ENV container=docker
ENV DEBIAN_FRONTEND=noninteractive

#
# systemd + minimal dependencies. We mask the units that don't make
# sense inside a container so they don't fail noisily on boot.
#

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        netcat-openbsd \
        iptables \
        iproute2 \
        systemd \
        systemd-sysv \
        sudo \
    && rm -rf /var/lib/apt/lists/* \
    && rm -f /lib/systemd/system/multi-user.target.wants/* \
             /etc/systemd/system/*.wants/* \
             /lib/systemd/system/local-fs.target.wants/* \
             /lib/systemd/system/sockets.target.wants/*udev* \
             /lib/systemd/system/sockets.target.wants/*initctl* \
             /lib/systemd/system/basic.target.wants/* \
    && systemctl mask -- \
        systemd-udevd.service \
        systemd-udevd-control.socket \
        systemd-udevd-kernel.socket \
        systemd-modules-load.service \
        sys-kernel-config.mount \
        sys-kernel-debug.mount \
        sys-kernel-tracing.mount \
        sys-fs-fuse-connections.mount \
        getty.target \
        console-getty.service \
        2>/dev/null || true

#
# Praxis user matches the systemd unit User=.
#

RUN groupadd -r praxis && \
    useradd  -r -g praxis -d /var/lib/praxis -s /usr/sbin/nologin praxis && \
    install -d -m 0750 -o praxis -g praxis /var/lib/praxis && \
    install -d -m 0755 /etc/praxis /usr/local/share/praxis/nodes

#
# Binaries.
#

COPY --from=builder /build/target/${CARGO_PROFILE}/praxis_service /usr/local/bin/
COPY --from=builder /build/target/${CARGO_PROFILE}/praxis_web     /usr/local/bin/
COPY --from=builder /build/target/${CARGO_PROFILE}/praxis_cli     /usr/local/bin/
RUN ln -sf praxis_cli /usr/local/bin/praxis

#
# Node binaries (for download / deployment to targets).
#

COPY --from=builder /build/target/${CARGO_PROFILE}/praxis_node                              /usr/local/share/praxis/nodes/praxis_node_linux
COPY --from=builder /build/target/x86_64-pc-windows-gnu/${CARGO_PROFILE}/praxis_node.exe    /usr/local/share/praxis/nodes/praxis_node_windows.exe

#
# systemd units, env file, praxisctl.
#

COPY pkg/systemd/praxis-service.service /etc/systemd/system/praxis-service.service
COPY pkg/systemd/praxis-web.service     /etc/systemd/system/praxis-web.service
COPY pkg/systemd/praxis.env.example     /etc/praxis/env
COPY pkg/praxisctl/praxisctl            /usr/local/bin/praxisctl
RUN chmod +x /usr/local/bin/praxisctl /usr/local/bin/praxis_service /usr/local/bin/praxis_web /usr/local/bin/praxis_cli

#
# Wait-for-rabbitmq oneshot so praxis-service doesn't crash-loop
# while the rabbitmq compose service is still starting.
#

COPY pkg/systemd/praxis-wait-rabbitmq.service /etc/systemd/system/praxis-wait-rabbitmq.service
COPY pkg/systemd/praxis-wait-rabbitmq.sh      /usr/local/bin/praxis-wait-rabbitmq

COPY pkg/systemd/docker-overrides/praxis-service-wait.conf /etc/systemd/system/praxis-service.service.d/wait.conf

RUN chmod +x /usr/local/bin/praxis-wait-rabbitmq && \
    systemctl enable praxis-wait-rabbitmq.service praxis-service.service praxis-web.service

ENV PRAXIS_NODES_DIR=/usr/local/share/praxis/nodes

EXPOSE 8080 8585 8586 8587

STOPSIGNAL SIGRTMIN+3

ENTRYPOINT ["/sbin/init"]
