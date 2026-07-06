#
# Praxis Docker Image
# Systemd-based runtime built from prebuilt release binaries.
#
# The runtime stage uses systemd as PID 1 so `praxisctl` works the
# same way inside the container as it does on a native Linux install.
#
# Binaries are downloaded from the GitHub release matching PRAXIS_VERSION.
# Override at build time, e.g.:
#
#   docker build --build-arg PRAXIS_VERSION=0.9.29 -t praxis .
#

FROM debian:bookworm-slim

ARG PRAXIS_VERSION=0.9.29
ARG PRAXIS_RELEASE_BASE=https://github.com/originsec/praxis/releases/download

ENV container=docker
ENV DEBIAN_FRONTEND=noninteractive

#
# systemd + minimal dependencies. We mask the units that don't make
# sense inside a container so they don't fail noisily on boot.
#

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
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
# Download and extract prebuilt release binaries.
#
# Tarball layout:
#   praxis-${PRAXIS_VERSION}-x86_64-linux/
#     praxis_service
#     praxis_cli
#     praxis_node
#     praxis_node_tiny_c
#     praxis_node_windows.exe
#     praxis_node_tiny_c_windows.exe
#     praxisctl
#     LICENSE
#

RUN set -eux; \
    tarball="praxis-${PRAXIS_VERSION}-x86_64-linux.tar.gz"; \
    url="${PRAXIS_RELEASE_BASE}/v${PRAXIS_VERSION}/${tarball}"; \
    curl -fsSL -o "/tmp/${tarball}" "$url"; \
    mkdir -p /tmp/praxis-extract; \
    tar -xzf "/tmp/${tarball}" -C /tmp/praxis-extract; \
    src="/tmp/praxis-extract/praxis-${PRAXIS_VERSION}-x86_64-linux"; \
    install -Dm755 "$src/praxis_service"                   /usr/local/bin/praxis_service; \
    install -Dm755 "$src/praxis_node"                      /usr/local/share/praxis/nodes/praxis_node_linux; \
    install -Dm644 "$src/praxis_node_windows.exe"          /usr/local/share/praxis/nodes/praxis_node_windows.exe; \
    install -Dm755 "$src/praxis_node_tiny_c"               /usr/local/share/praxis/nodes/praxis_node_tiny_c_linux; \
    install -Dm644 "$src/praxis_node_tiny_c_windows.exe"   /usr/local/share/praxis/nodes/praxis_node_tiny_c_windows.exe; \
    rm -rf "/tmp/${tarball}" /tmp/praxis-extract

#
# systemd units, env file, praxisctl. These ship with the source tree
# rather than the release tarball, so we copy them from the build context.
#

COPY pkg/systemd/praxis-service.service /etc/systemd/system/praxis-service.service
COPY pkg/systemd/praxis.env.example     /etc/praxis/env
COPY pkg/praxisctl/praxisctl            /usr/local/bin/praxisctl
RUN sed -i 's/\r$//' /usr/local/bin/praxisctl && \
    chmod +x /usr/local/bin/praxisctl /usr/local/bin/praxis_service && \
    sed -i 's|@localhost:5672|@rabbitmq:5672|' /etc/praxis/env

#
# Wait-for-rabbitmq oneshot so praxis-service doesn't crash-loop
# while the rabbitmq compose service is still starting.
#

COPY pkg/systemd/praxis-wait-rabbitmq.service /etc/systemd/system/praxis-wait-rabbitmq.service
COPY pkg/systemd/praxis-wait-rabbitmq.sh      /usr/local/bin/praxis-wait-rabbitmq

COPY pkg/systemd/docker-overrides/praxis-service-wait.conf /etc/systemd/system/praxis-service.service.d/wait.conf

RUN chmod +x /usr/local/bin/praxis-wait-rabbitmq && \
    systemctl enable praxis-wait-rabbitmq.service praxis-service.service

ENV PRAXIS_NODES_DIR=/usr/local/share/praxis/nodes

EXPOSE 8585 8586 8587

STOPSIGNAL SIGRTMIN+3

ENTRYPOINT ["/sbin/init"]
