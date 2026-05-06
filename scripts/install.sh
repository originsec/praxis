#!/usr/bin/env bash
#
# Praxis Installation Script
# Usage: curl -fsSL https://praxis.originhq.com/install.sh | bash
#
# Linux and macOS only. Windows users should use install.ps1.
#
# The installer first asks which components to install:
#   [x] service   (Linux only — installs praxis-service + praxis-web
#                  systemd units, /etc/praxis/env, praxisctl)
#   [x] cli       (always native; installs `praxis` to /usr/local/bin)
#
# Then asks how to install the service:
#   - native      (Linux only; system-wide systemd, requires RabbitMQ)
#   - docker      (Linux + macOS; rabbitmq + praxis container)
#
# Non-interactive flags:
#   --service [native|docker]    Install the service in the chosen mode
#   --cli                        Install the CLI natively
#   --remove                     Remove all native + docker installs
#   --help                       Show usage
#

set -e

PRAXIS_REPO="originsec/praxis"
PRAXIS_VERSION="${PRAXIS_VERSION:-}"

PRAXIS_DOCKER_DIR="${PRAXIS_DIR:-$HOME/.praxis-docker}"

INSTALL_PREFIX="${INSTALL_PREFIX:-/usr/local}"
INSTALL_BIN="$INSTALL_PREFIX/bin"
INSTALL_SHARE="$INSTALL_PREFIX/share/praxis"

OS_KIND=""        # linux | macos
HAS_DOCKER=false
COMPOSE_CMD=""

#
# Colors.
#

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

info()    { echo -e "${CYAN}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $1"; }
error()   { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
has_cmd() { command -v "$1" &> /dev/null; }

print_banner() {
    local cols logo_w=46 tag_w=49 by_w=13
    cols=$(tput cols 2>/dev/null || echo 80)
    local lp=$(( (cols - logo_w) / 2 ))
    local tp=$(( (cols - tag_w) / 2 ))
    local bp=$(( (cols - by_w) / 2 ))
    (( lp < 0 )) && lp=0
    (( tp < 0 )) && tp=0
    (( bp < 0 )) && bp=0
    local lpad tpad bpad
    lpad=$(printf '%*s' "$lp" '')
    tpad=$(printf '%*s' "$tp" '')
    bpad=$(printf '%*s' "$bp" '')

    echo
    printf '%b' "${CYAN}${BOLD}"
    echo "${lpad}██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗"
    echo "${lpad}██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝"
    echo "${lpad}██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗"
    echo "${lpad}██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║"
    echo "${lpad}██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║"
    echo "${lpad}╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝"
    printf '%b' "${NC}"
    printf '%b\n' "${tpad}${DIM}Semantic Command & Control Framework for Agents${NC}"
    printf '%b\n' "${bpad}${MAGENTA}by [Ø] Origin${NC}"
    echo
}

usage() {
    cat <<EOF
Usage: install.sh [flag]

Flags:
  --service [native|docker]   Install the service in the chosen mode
  --cli                       Install the CLI natively
  --remove                    Remove a previous install (native + docker)
  --help                      Show this message

If no flag is given, an interactive menu is shown.
EOF
}

#
# Platform detection.
#

detect_platform() {
    local os
    os="$(uname -s 2>/dev/null || echo unknown)"
    case "$os" in
        Linux)  OS_KIND="linux" ;;
        Darwin) OS_KIND="macos" ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            error "Windows is not supported by install.sh. Use install.ps1 instead."
            ;;
        *)
            OS_KIND="linux"
            warn "Unknown OS '$os' - assuming Linux."
            ;;
    esac
}

#
# === Arrow-key menus =========================================================
#
# Reads from /dev/tty so menus work under `curl | bash`.
#

SELECTED=0
select_menu() {
    local prompt="$1"; shift
    local options=("$@")
    local n=${#options[@]}
    local sel=0
    local tty_in=/dev/tty
    local tty_out=/dev/tty

    if [[ ! -e /dev/tty ]]; then
        echo "$prompt" >&2
        for i in "${!options[@]}"; do echo "  $((i+1))) ${options[$i]}" >&2; done
        error "No TTY available. Re-run with: --service native|docker, --cli, or --remove"
    fi

    printf '\033[?25l' > "$tty_out"
    trap 'printf "\033[?25h" > '"$tty_out"'; stty echo 2>/dev/null || true' EXIT INT TERM

    printf "%b %b\n" "$prompt" "${DIM}(↑↓ move • enter select • q quit)${NC}" > "$tty_out"
    for ((i=0; i<n; i++)); do printf "\n" > "$tty_out"; done

    while true; do
        printf "\033[%dA" "$n" > "$tty_out"
        for ((i=0; i<n; i++)); do
            if (( i == sel )); then
                printf "\r\033[K  ${CYAN}${BOLD}▶ %s${NC}\n" "${options[$i]}" > "$tty_out"
            else
                printf "\r\033[K    ${DIM}%s${NC}\n" "${options[$i]}" > "$tty_out"
            fi
        done

        local key=""
        IFS= read -rsn1 key < "$tty_in" || true

        if [[ $key == $'\x1b' ]]; then
            local rest=""
            IFS= read -rsn2 -t 0.05 rest < "$tty_in" || true
            case "$rest" in
                '[A'|'OA') sel=$(( (sel - 1 + n) % n ));;
                '[B'|'OB') sel=$(( (sel + 1) % n ));;
            esac
        elif [[ -z $key ]]; then
            break
        elif [[ $key == $'\n' || $key == $'\r' ]]; then
            break
        elif [[ $key =~ ^[0-9]$ ]]; then
            local idx=$((key - 1))
            if (( idx >= 0 && idx < n )); then sel=$idx; break; fi
        elif [[ $key == "k" ]]; then
            sel=$(( (sel - 1 + n) % n ))
        elif [[ $key == "j" ]]; then
            sel=$(( (sel + 1) % n ))
        elif [[ $key == "q" || $key == $'\x03' ]]; then
            printf '\033[?25h' > "$tty_out"
            echo > "$tty_out"
            exit 130
        fi
    done

    printf '\033[?25h' > "$tty_out"
    trap - EXIT INT TERM
    SELECTED=$sel
}

#
# Multi-select (checkbox) menu. Args: prompt, [pre-checked-flags...], options...
# pre-checked flags are 1/0 in same order as options; bundled in $CHECKED.
#

CHECKED=()
multi_select_menu() {
    local prompt="$1"; shift
    local n_opts=$1; shift
    local checked=()
    for ((i=0; i<n_opts; i++)); do checked+=("${1:-1}"); shift; done
    local options=("$@")
    local sel=0
    local tty_in=/dev/tty
    local tty_out=/dev/tty

    if [[ ! -e /dev/tty ]]; then
        error "No TTY for component selection. Re-run with: --service [native|docker] and/or --cli"
    fi

    printf '\033[?25l' > "$tty_out"
    trap 'printf "\033[?25h" > '"$tty_out"'; stty echo 2>/dev/null || true' EXIT INT TERM

    printf "%b %b\n" "$prompt" "${DIM}(↑↓ move • space toggle • enter next • q quit)${NC}" > "$tty_out"
    for ((i=0; i<n_opts; i++)); do printf "\n" > "$tty_out"; done

    while true; do
        printf "\033[%dA" "$n_opts" > "$tty_out"
        for ((i=0; i<n_opts; i++)); do
            local mark="[ ]"
            (( checked[i] )) && mark="${CYAN}${BOLD}[x]${NC}"
            if (( i == sel )); then
                printf "\r\033[K  ${CYAN}${BOLD}▶${NC} %b %s\n" "$mark" "${options[$i]}" > "$tty_out"
            else
                printf "\r\033[K    %b %s\n" "$mark" "${options[$i]}" > "$tty_out"
            fi
        done

        local key=""
        IFS= read -rsn1 key < "$tty_in" || true

        if [[ $key == $'\x1b' ]]; then
            local rest=""
            IFS= read -rsn2 -t 0.05 rest < "$tty_in" || true
            case "$rest" in
                '[A'|'OA') sel=$(( (sel - 1 + n_opts) % n_opts ));;
                '[B'|'OB') sel=$(( (sel + 1) % n_opts ));;
            esac
        elif [[ -z $key || $key == $'\n' || $key == $'\r' ]]; then
            break
        elif [[ $key == " " ]]; then
            checked[sel]=$(( 1 - checked[sel] ))
        elif [[ $key == "k" ]]; then
            sel=$(( (sel - 1 + n_opts) % n_opts ))
        elif [[ $key == "j" ]]; then
            sel=$(( (sel + 1) % n_opts ))
        elif [[ $key == "q" || $key == $'\x03' ]]; then
            printf '\033[?25h' > "$tty_out"
            echo > "$tty_out"
            exit 130
        fi
    done

    printf '\033[?25h' > "$tty_out"
    trap - EXIT INT TERM
    CHECKED=("${checked[@]}")
}

#
# === Version resolution =====================================================
#

get_latest_version() {
    if [ -n "$PRAXIS_VERSION" ]; then
        success "Using specified version: $PRAXIS_VERSION"
        echo ""
        return
    fi

    info "Fetching latest release version..."
    PRAXIS_VERSION=$(curl -fsSL "https://api.github.com/repos/$PRAXIS_REPO/releases/latest" | \
        grep '"tag_name":' | \
        sed 's/.*"tag_name": "\([^"]*\)".*/\1/')

    if [ -z "$PRAXIS_VERSION" ]; then
        error "Could not determine latest version. Check your internet connection."
    fi

    success "Latest version: $PRAXIS_VERSION"
    echo ""
}

#
# === Sudo helper ============================================================
#

SUDO=""
ensure_sudo() {
    if [[ $EUID -eq 0 ]]; then
        SUDO=""
    elif has_cmd sudo; then
        SUDO="sudo"
    else
        error "Need root to install system files; sudo not available."
    fi
}

#
# === RabbitMQ checks (native service install) ===============================
#

check_rabbitmq_or_die() {
    info "Checking RabbitMQ..."
    if has_cmd rabbitmqctl && rabbitmqctl status >/dev/null 2>&1; then
        success "RabbitMQ is running"
    elif has_cmd systemctl && systemctl is-active --quiet rabbitmq-server 2>/dev/null; then
        success "RabbitMQ service is active"
    else
        warn "RabbitMQ does not appear to be installed/running."
        echo "Praxis (native install) requires a running RabbitMQ broker."
        echo "Install RabbitMQ first, e.g.:"
        echo "  - Debian/Ubuntu:  sudo apt-get install rabbitmq-server"
        echo "  - Fedora/RHEL:    sudo dnf install rabbitmq-server"
        echo "  - Arch:           sudo pacman -S rabbitmq"
        echo "Then ensure it's running:"
        echo "  sudo systemctl enable --now rabbitmq-server"
        echo ""
        error "Aborting native install."
    fi
}

ensure_rabbitmq_user() {
    if ! has_cmd rabbitmqctl; then
        warn "rabbitmqctl not found - skipping RabbitMQ user setup."
        return
    fi
    if $SUDO rabbitmqctl list_users 2>/dev/null | awk '{print $1}' | grep -qx 'praxis'; then
        success "RabbitMQ user 'praxis' already exists"
    else
        info "Creating RabbitMQ user 'praxis'..."
        $SUDO rabbitmqctl add_user praxis praxis >/dev/null
        $SUDO rabbitmqctl set_permissions praxis ".*" ".*" ".*" >/dev/null
        $SUDO rabbitmqctl set_user_tags praxis administrator >/dev/null 2>&1 || true
        success "Created RabbitMQ user 'praxis'"
    fi
}

#
# === Native CLI install =====================================================
#

check_rust() {
    if has_cmd cargo; then
        success "Found Rust $(rustc --version 2>/dev/null | cut -d' ' -f2)"
    else
        warn "Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck disable=SC1090
        source "$HOME/.cargo/env"
        has_cmd cargo || error "Failed to install Rust"
        success "Rust installed"
    fi
    local rmajor rminor
    rmajor=$(rustc --version | sed 's/rustc \([0-9]*\)\.\([0-9]*\).*/\1/')
    rminor=$(rustc --version | sed 's/rustc \([0-9]*\)\.\([0-9]*\).*/\2/')
    if [[ "$rmajor" -lt 1 ]] || [[ "$rmajor" -eq 1 && "$rminor" -lt 85 ]]; then
        warn "Rust 1.85+ required. Updating..."
        rustup update stable
    fi
}

install_cli_native() {
    info "Installing Praxis CLI to $INSTALL_BIN/praxis..."
    has_cmd git || error "git not found. Please install git."
    check_rust

    local repo_url="https://github.com/$PRAXIS_REPO"
    local tmproot
    tmproot=$(mktemp -d)
    cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$tmproot" praxis_cli

    ensure_sudo
    $SUDO install -d "$INSTALL_BIN"
    $SUDO install -m 0755 "$tmproot/bin/praxis_cli" "$INSTALL_BIN/praxis_cli"
    $SUDO ln -sf praxis_cli "$INSTALL_BIN/praxis"
    rm -rf "$tmproot"
    success "Installed CLI: $INSTALL_BIN/praxis (and praxis_cli)"
    echo ""
}

#
# === Native service install (Linux only) ====================================
#

install_service_native() {
    [[ "$OS_KIND" == "linux" ]] || error "Native service install is Linux-only. Use docker instead."
    has_cmd systemctl || error "systemctl not found - native install requires systemd."
    has_cmd git || error "git not found. Please install git."

    ensure_sudo
    check_rabbitmq_or_die
    check_rust

    info "Building praxis_service, praxis_web, and praxis_node..."
    local repo_url="https://github.com/$PRAXIS_REPO"
    local tmproot
    tmproot=$(mktemp -d)
    cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$tmproot" praxis_service praxis_web praxis_node
    success "Built service binaries"

    info "Installing system files..."
    $SUDO install -d "$INSTALL_BIN" "$INSTALL_SHARE/nodes" /etc/praxis /var/lib/praxis

    if ! getent group praxis >/dev/null 2>&1; then
        $SUDO groupadd -r praxis
    fi
    if ! id -u praxis >/dev/null 2>&1; then
        $SUDO useradd -r -g praxis -d /var/lib/praxis -s /usr/sbin/nologin praxis
    fi
    $SUDO chown praxis:praxis /var/lib/praxis
    $SUDO chmod 0750 /var/lib/praxis

    $SUDO install -m 0755 "$tmproot/bin/praxis_service" "$INSTALL_BIN/praxis_service"
    $SUDO install -m 0755 "$tmproot/bin/praxis_web"     "$INSTALL_BIN/praxis_web"
    $SUDO install -m 0755 "$tmproot/bin/praxis_node"    "$INSTALL_SHARE/nodes/praxis_node_linux"
    rm -rf "$tmproot"

    info "Fetching unit files and praxisctl..."
    local pkg_tmp
    pkg_tmp=$(mktemp -d)
    git clone --depth 1 --branch "$PRAXIS_VERSION" "$repo_url" "$pkg_tmp/repo" >/dev/null 2>&1

    $SUDO install -m 0644 "$pkg_tmp/repo/pkg/systemd/praxis-service.service" /etc/systemd/system/praxis-service.service
    $SUDO install -m 0644 "$pkg_tmp/repo/pkg/systemd/praxis-web.service"     /etc/systemd/system/praxis-web.service

    if [[ ! -f /etc/praxis/env ]]; then
        $SUDO install -m 0640 "$pkg_tmp/repo/pkg/systemd/praxis.env.example" /etc/praxis/env
        $SUDO chgrp praxis /etc/praxis/env 2>/dev/null || true
    else
        info "/etc/praxis/env already exists - leaving in place"
    fi

    $SUDO install -m 0755 "$pkg_tmp/repo/pkg/praxisctl/praxisctl" "$INSTALL_BIN/praxisctl"
    rm -rf "$pkg_tmp"

    ensure_rabbitmq_user

    $SUDO systemctl daemon-reload
    info "Enabling praxis-service and praxis-web..."
    $SUDO systemctl enable --now praxis-service.service
    $SUDO systemctl enable --now praxis-web.service

    success "Installed native service. Check status with: praxisctl status"
    echo ""
}

print_native_summary() {
    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Praxis $PRAXIS_VERSION installed"
    echo "=============================================="
    echo -e "${NC}"
    echo "Binaries:    $INSTALL_BIN/{praxis_service,praxis_web,praxis_cli,praxis,praxisctl}"
    echo "Config:      /etc/praxis/env"
    echo "Data:        /var/lib/praxis"
    echo "Node binary: $INSTALL_SHARE/nodes/praxis_node_linux"
    echo ""
    echo -e "${CYAN}Service control:${NC}"
    echo "  praxisctl status           # praxis-service status"
    echo "  praxisctl logs -f          # praxis-service logs"
    echo "  praxisctl webserver status # praxis-web status"
    echo "  praxisctl set rabbitmq-url amqp://praxis:praxis@localhost:5672"
    echo ""
    echo -e "${CYAN}CLI:${NC}"
    echo "  praxis                     # interactive TUI"
    echo "  praxis set-rabbitmqurl <url>"
    echo ""
    echo "Web UI: http://localhost:8080"
    echo ""
}

#
# === Docker service install =================================================
#

check_docker() {
    info "Checking Docker..."
    has_cmd docker || error "Docker not found. Please install Docker: https://docs.docker.com/get-docker/"
    docker info >/dev/null 2>&1 || error "Docker daemon not running. Start Docker first."
    success "Docker daemon running"

    if docker compose version >/dev/null 2>&1; then
        COMPOSE_CMD="docker compose"
    elif has_cmd docker-compose; then
        COMPOSE_CMD="docker-compose"
    else
        error "Docker Compose not found. Install Docker Desktop / docker compose plugin."
    fi
    success "Found $COMPOSE_CMD"
    has_cmd git || error "git not found. Please install git."
    echo ""
}

install_service_docker() {
    check_docker
    info "Setting up Praxis $PRAXIS_VERSION in $PRAXIS_DOCKER_DIR..."
    rm -rf "$PRAXIS_DOCKER_DIR"
    git clone --depth 1 --branch "$PRAXIS_VERSION" "https://github.com/$PRAXIS_REPO.git" "$PRAXIS_DOCKER_DIR"

    info "Building and starting (this may take a few minutes on first run)..."
    ( cd "$PRAXIS_DOCKER_DIR" && $COMPOSE_CMD up --build -d )
    success "Praxis is running"
    echo ""
}

print_docker_summary() {
    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Praxis $PRAXIS_VERSION (docker) is ready"
    echo "=============================================="
    echo -e "${NC}"
    echo "Web UI:              http://localhost:8080"
    echo "RabbitMQ Management: http://localhost:15672 (praxis / praxis)"
    echo "Installation:        $PRAXIS_DOCKER_DIR"
    echo ""
    echo -e "${CYAN}Inside the container:${NC}"
    echo "  $COMPOSE_CMD exec praxis praxisctl status"
    echo "  $COMPOSE_CMD exec praxis praxisctl webserver disable"
    echo "  $COMPOSE_CMD exec praxis praxisctl set rabbitmq-url <url>"
    echo ""
    echo -e "${CYAN}Compose lifecycle:${NC}"
    echo "  cd $PRAXIS_DOCKER_DIR"
    echo "  $COMPOSE_CMD logs -f"
    echo "  $COMPOSE_CMD down"
    echo "  $COMPOSE_CMD up -d"
    echo ""
}

#
# === Remove =================================================================
#

remove_native() {
    info "Removing native install..."
    if has_cmd systemctl; then
        $SUDO systemctl disable --now praxis-web.service     2>/dev/null || true
        $SUDO systemctl disable --now praxis-service.service 2>/dev/null || true
        $SUDO rm -f /etc/systemd/system/praxis-service.service \
                    /etc/systemd/system/praxis-web.service
        $SUDO systemctl daemon-reload 2>/dev/null || true
    fi
    $SUDO rm -f "$INSTALL_BIN/praxis_service" \
                "$INSTALL_BIN/praxis_web" \
                "$INSTALL_BIN/praxis_cli" \
                "$INSTALL_BIN/praxis" \
                "$INSTALL_BIN/praxisctl"
    $SUDO rm -rf "$INSTALL_SHARE"
    if [[ "${PRAXIS_REMOVE_DATA:-0}" = "1" ]]; then
        $SUDO rm -rf /var/lib/praxis /etc/praxis
    else
        echo "Leaving /etc/praxis and /var/lib/praxis in place."
        echo "Set PRAXIS_REMOVE_DATA=1 to also remove config and database."
    fi
    success "Native install removed"
}

remove_docker() {
    if [[ -d "$PRAXIS_DOCKER_DIR" ]]; then
        info "Removing docker install at $PRAXIS_DOCKER_DIR..."
        if has_cmd docker; then
            ( cd "$PRAXIS_DOCKER_DIR" && \
                ( docker compose down -v 2>/dev/null || docker-compose down -v 2>/dev/null || true ) )
        fi
        rm -rf "$PRAXIS_DOCKER_DIR"
        success "Docker install removed"
    else
        info "No docker install at $PRAXIS_DOCKER_DIR"
    fi
}

remove_all() {
    ensure_sudo
    remove_native
    remove_docker
    echo ""
    success "Praxis has been removed."
}

#
# === Interactive flow =======================================================
#

interactive_install() {
    multi_select_menu "${BOLD}Choose components to install${NC}" 2 1 1 \
        "service   - Praxis service + web" \
        "cli       - Praxis CLI"

    local want_service=${CHECKED[0]}
    local want_cli=${CHECKED[1]}
    echo

    if (( want_service == 0 && want_cli == 0 )); then
        error "Nothing selected."
    fi

    local service_mode=""
    if (( want_service )); then
        local options=()
        local actions=()
        if [[ "$OS_KIND" == "linux" ]]; then
            options+=("Native install   - system-wide systemd (requires RabbitMQ)")
            actions+=("native")
        fi
        options+=("Docker install   - rabbitmq + service in containers")
        actions+=("docker")
        options+=("Cancel")
        actions+=("cancel")

        select_menu "${BOLD}Install service as${NC}" "${options[@]}"
        service_mode="${actions[$SELECTED]}"
        echo
        case "$service_mode" in
            cancel) error "Aborted." ;;
        esac
    fi

    get_latest_version

    if (( want_cli )); then
        install_cli_native
    fi
    if (( want_service )); then
        case "$service_mode" in
            native) install_service_native; print_native_summary ;;
            docker) install_service_docker; print_docker_summary ;;
        esac
    elif (( want_cli )); then
        echo -e "${GREEN}CLI installed.${NC} Run: praxis"
    fi
}

#
# === Flag dispatch ==========================================================
#

main() {
    print_banner
    detect_platform

    case "${1:-}" in
        --help|-h)
            usage; exit 0 ;;
        --remove)
            remove_all; exit 0 ;;
        --cli)
            get_latest_version
            install_cli_native
            success "CLI installed. Run: praxis"
            exit 0 ;;
        --service)
            local mode="${2:-}"
            [[ -n "$mode" ]] || error "--service requires native|docker"
            get_latest_version
            case "$mode" in
                native) install_service_native; print_native_summary ;;
                docker) install_service_docker; print_docker_summary ;;
                *) error "Unknown service mode: $mode" ;;
            esac
            exit 0 ;;
        "")
            interactive_install ;;
        *)
            usage; exit 1 ;;
    esac
}

main "$@"
