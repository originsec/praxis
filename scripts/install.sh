#!/usr/bin/env bash
#
# Praxis Installation Script
# Usage: curl -fsSL https://praxis.originhq.com/install.sh | bash
#
# Linux and macOS only. Windows users should use install.ps1.
#
# The installer asks how to install the service:
#   - native       (Linux only; system-wide systemd, requires RabbitMQ)
#   - docker       (Linux + macOS; rabbitmq + praxis container)
#   - client only  (no service; just the praxis CLI)
#
# The CLI (`praxis`) is always installed natively to /usr/local/bin
# regardless of the chosen option.
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

info()    { echo -e "  ${CYAN}▸${NC} $1"; }
success() { echo -e "  ${GREEN}✓${NC} $1"; }
warn()    { echo -e "  ${YELLOW}⚠${NC} $1"; }
error()   { echo -e "  ${RED}✗${NC} $1"; exit 1; }
has_cmd() { command -v "$1" &> /dev/null; }

section() {
    local title="$1"
    echo
    printf "  %b▌%b %b%s%b\n\n" "${CYAN}${BOLD}" "${NC}" "${BOLD}" "$title" "${NC}"
}

run_with_progress_bar() {
    local logfile="$1"
    shift
    local cmd=("$@")

    "${cmd[@]}" >"$logfile" 2>&1 &
    local pid=$!

    local width=40
    local percent=0
    local step=2
    local delay=0.3
    local spin_chars=(⣾ ⣽ ⣻ ⢿ ⡿ ⣟ ⣯ ⣷)
    local spin_idx=0

    while kill -0 $pid 2>/dev/null; do
        local filled=$(( percent * width / 100 ))
        local empty=$(( width - filled ))
        local fill_bar=""
        local empty_bar=""
        for ((i=0; i<filled; i++)); do fill_bar+="█"; done
        for ((i=0; i<empty; i++)); do empty_bar+="░"; done
        printf "\r\033[K${CYAN}[${BOLD}%s${NC}${DIM}%s${NC}${CYAN}] %3d%% ${spin_chars[$spin_idx]}${NC}" "$fill_bar" "$empty_bar" "$percent"
        spin_idx=$(( (spin_idx + 1) % ${#spin_chars[@]} ))
        sleep "$delay"
        percent=$(( percent + step ))
        if (( percent > 95 )); then percent=95; fi
    done

    local exit_code=0
    wait $pid || exit_code=$?

    local bar=""
    for ((i=0; i<width; i++)); do bar+="█"; done
    printf "\r\033[K${CYAN}[${BOLD}%s${NC}${CYAN}] 100%%${NC}\n" "$bar"

    return $exit_code
}

cleanup_and_exit() {
    local job
    for job in $(jobs -p); do
        kill -TERM "$job" 2>/dev/null || true
        sleep 0.1
        kill -KILL "$job" 2>/dev/null || true
    done
    printf '\033[?25h' 2>/dev/null || true
    stty echo 2>/dev/null || true
    echo ""
    exit 130
}
trap cleanup_and_exit INT

print_banner() {
    echo
    printf '%b' "${CYAN}${BOLD}"
    echo "██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗"
    echo "██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝"
    echo "██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗"
    echo "██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║"
    echo "██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║"
    echo "╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝"
    printf '%b' "${NC}"
    printf '%b\n' "${DIM}Semantic Command & Control Framework for Agents${NC} ${MAGENTA}by [Ø] Origin${NC}"
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
MENU_FOOTER=""
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
    trap 'printf "\033[?25h" > '"$tty_out"'; stty echo 2>/dev/null || true' EXIT

    printf "%b %b\n\n" "$prompt" "${DIM}(↑↓ move • enter select • q quit)${NC}" > "$tty_out"
    for ((i=0; i<n; i++)); do printf "\n" > "$tty_out"; done

    #
    # Render an optional footer below the option list (e.g. an info note).
    # Printed once and then we move the cursor back up onto the option area
    # so the redraw loop never overwrites it.
    #

    local footer_lines=0
    if [[ -n "$MENU_FOOTER" ]]; then
        printf "\n%b\n" "$MENU_FOOTER" > "$tty_out"
        footer_lines=2
        printf "\033[%dA" "$footer_lines" > "$tty_out"
    fi

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
            (( footer_lines > 0 )) && printf "\033[%dB" "$footer_lines" > "$tty_out"
            echo > "$tty_out"
            exit 130
        fi
    done

    (( footer_lines > 0 )) && printf "\033[%dB" "$footer_lines" > "$tty_out"
    printf '\033[?25h' > "$tty_out"
    trap - EXIT
    SELECTED=$sel
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
        if ! sudo -n true 2>/dev/null; then
            info "Root privileges required. Please enter your password."
            sudo -v || error "Failed to authenticate with sudo."
        fi
    else
        error "Need root to install system files; sudo not available."
    fi
}

#
# === RabbitMQ setup (runs after native install) ===========================
#

setup_rabbitmq_or_warn() {
    if ! has_cmd rabbitmqctl; then
        warn "rabbitmqctl not found. Skipping RabbitMQ user setup."
        echo "Ensure RabbitMQ is installed and a 'praxis' user exists before starting the service."
        return
    fi

    info "Checking RabbitMQ..."
    if ! $SUDO rabbitmqctl status >/dev/null 2>&1; then
        warn "RabbitMQ does not appear to be running."
        echo "Praxis requires a running RabbitMQ broker."
        echo "Install and start RabbitMQ, then create the user manually:"
        echo "  sudo rabbitmqctl add_user praxis praxis"
        echo "  sudo rabbitmqctl set_permissions praxis '.*' '.*' '.*'"
        echo "  sudo rabbitmqctl set_user_tags praxis administrator"
        return
    fi

    success "RabbitMQ is running"

    if $SUDO rabbitmqctl list_users 2>/dev/null | awk '{print $1}' | grep -qx 'praxis'; then
        success "RabbitMQ user 'praxis' already exists"
        return
    fi

    info "Creating RabbitMQ user 'praxis'..."
    if $SUDO rabbitmqctl add_user praxis praxis >/dev/null 2>&1 && \
       $SUDO rabbitmqctl set_permissions praxis ".*" ".*" ".*" >/dev/null 2>&1; then
        $SUDO rabbitmqctl set_user_tags praxis administrator >/dev/null 2>&1 || true
        success "Created RabbitMQ user 'praxis'"
    else
        warn "Failed to create RabbitMQ user 'praxis'."
        echo "Create it manually before starting the service:"
        echo "  sudo rabbitmqctl add_user praxis praxis"
        echo "  sudo rabbitmqctl set_permissions praxis '.*' '.*' '.*'"
        echo "  sudo rabbitmqctl set_user_tags praxis administrator"
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

get_local_binary() {
    local name="$1"
    local script_dir=""
    if [[ -n "${BASH_SOURCE[0]}" && "${BASH_SOURCE[0]}" != "-" && "${BASH_SOURCE[0]}" != "/dev/stdin" ]]; then
        script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
    fi
    if [[ -n "$script_dir" && -x "$script_dir/../target/release/$name" ]]; then
        echo "$script_dir/../target/release/$name"
        return 0
    fi
    return 1
}

install_cli_native() {
    section "Installing CLI"
    has_cmd git || error "git not found. Please install git."

    local repo_url="https://github.com/$PRAXIS_REPO"
    local tmproot
    tmproot=$(mktemp -d)

    local binary_path
    if binary_path=$(get_local_binary "praxis_cli"); then
        success "Using locally compiled binary: $binary_path"
        mkdir -p "$tmproot/bin"
        cp "$binary_path" "$tmproot/bin/praxis_cli"
    else
        check_rust
        local cargo_log="$tmproot/cargo.log"
        if ! run_with_progress_bar "$cargo_log" \
                cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$tmproot" praxis_cli; then
            echo ""
            warn "Build output follows (last 50 lines):"
            tail -n 50 "$cargo_log"
            error "cargo install failed for CLI."
        fi
    fi

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
    section "Installing Service"

    local repo_url="https://github.com/$PRAXIS_REPO"
    local tmproot
    tmproot=$(mktemp -d)

    local svc_path web_path node_path
    if svc_path=$(get_local_binary "praxis_service") && \
       web_path=$(get_local_binary "praxis_web") && \
       node_path=$(get_local_binary "praxis_node"); then
        success "Using locally compiled binaries"
        mkdir -p "$tmproot/bin"
        cp "$svc_path" "$tmproot/bin/praxis_service"
        cp "$web_path" "$tmproot/bin/praxis_web"
        cp "$node_path" "$tmproot/bin/praxis_node"
    else
        check_rust
        info "Building praxis_service, praxis_web, and praxis_node..."
        local cargo_log="$tmproot/cargo.log"
        if ! run_with_progress_bar "$cargo_log" \
                cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$tmproot" \
                praxis_service praxis_web praxis_node; then
            echo ""
            warn "Build output follows (last 50 lines):"
            tail -n 50 "$cargo_log"
            error "cargo install failed for service binaries."
        fi
        success "Built service binaries"
    fi

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
    local repo_dir=""
    local pkg_tmp=""

    # Detect if we're running from within the praxis repo
    local script_dir=""
    if [[ -n "${BASH_SOURCE[0]}" && "${BASH_SOURCE[0]}" != "-" && "${BASH_SOURCE[0]}" != "/dev/stdin" ]]; then
        script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
    fi

    if [[ -n "$script_dir" && -f "$script_dir/../pkg/systemd/praxis-service.service" && -f "$script_dir/../pkg/praxisctl/praxisctl" ]]; then
        info "Using local repository files..."
        repo_dir="$script_dir/.."
    else
        pkg_tmp=$(mktemp -d)
        repo_dir="$pkg_tmp/repo"
        git clone --depth 1 --branch "$PRAXIS_VERSION" "$repo_url" "$repo_dir" || error "Failed to clone repository. Check your internet connection and version tag: $PRAXIS_VERSION"
    fi

    $SUDO install -m 0644 "$repo_dir/pkg/systemd/praxis-service.service" /etc/systemd/system/praxis-service.service
    $SUDO install -m 0644 "$repo_dir/pkg/systemd/praxis-web.service"     /etc/systemd/system/praxis-web.service

    if [[ ! -f /etc/praxis/env ]]; then
        $SUDO install -m 0640 "$repo_dir/pkg/systemd/praxis.env.example" /etc/praxis/env
        $SUDO chgrp praxis /etc/praxis/env 2>/dev/null || true
    else
        info "/etc/praxis/env already exists - leaving in place"
    fi

    $SUDO install -m 0755 "$repo_dir/pkg/praxisctl/praxisctl" "$INSTALL_BIN/praxisctl"
    [[ -n "$pkg_tmp" ]] && rm -rf "$pkg_tmp"

    setup_rabbitmq_or_warn

    $SUDO systemctl daemon-reload
    info "Enabling praxis-service and praxis-web..."
    $SUDO systemctl enable --now praxis-service.service
    $SUDO systemctl enable --now praxis-web.service

    success "Installed native service. Check status with: praxisctl status"
    echo ""
}

print_summary_box() {
    local title="$1"
    local inner=46
    local title_len=${#title}
    local pad=$(( (inner - title_len) / 2 ))
    local lpad rpad
    lpad=$(printf '%*s' "$pad" '')
    rpad=$(printf '%*s' "$(( inner - title_len - pad ))" '')
    local hbar
    hbar=$(printf '─%.0s' $(seq 1 "$inner"))
    echo
    printf "  %b╭%s╮%b\n" "$GREEN" "$hbar" "$NC"
    printf "  %b│%b%s%b%s%b%s%b│%b\n" "$GREEN" "$NC" "$lpad" "${GREEN}${BOLD}" "$title" "$NC" "$rpad" "$GREEN" "$NC"
    printf "  %b╰%s╯%b\n" "$GREEN" "$hbar" "$NC"
    echo
}

print_native_summary() {
    print_summary_box "Praxis $PRAXIS_VERSION installed"
    printf "  %bBinaries${NC}    %s/{praxis_service,praxis_web,praxis_cli,praxis,praxisctl}\n" "${BOLD}" "$INSTALL_BIN"
    printf "  %bConfig${NC}      /etc/praxis/env\n" "${BOLD}"
    printf "  %bData${NC}        /var/lib/praxis\n" "${BOLD}"
    printf "  %bNode binary${NC} %s/nodes/praxis_node_linux\n" "${BOLD}" "$INSTALL_SHARE"
    echo
    printf "  %bService control${NC}\n" "${CYAN}${BOLD}"
    echo   "    praxisctl status            ${DIM}# praxis-service status${NC}"
    echo   "    praxisctl webserver status  ${DIM}# praxis-web status${NC}"
    echo   "    praxisctl set-rabbitmqurl amqp://praxis:praxis@localhost:5672"
    echo
    printf "  %bCLI${NC}\n" "${CYAN}${BOLD}"
    echo   "    praxis                      ${DIM}# interactive TUI${NC}"
    echo
    printf "  %bWeb UI${NC}      http://localhost:8080\n" "${BOLD}"
    echo
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
    section "Installing Service (Docker)"
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
    print_summary_box "Praxis $PRAXIS_VERSION (docker) ready"
    printf "  %bWeb UI${NC}              http://localhost:8080\n" "${BOLD}"
    printf "  %bRabbitMQ Management${NC} http://localhost:15672 ${DIM}(praxis / praxis)${NC}\n" "${BOLD}"
    printf "  %bInstallation${NC}        %s\n" "${BOLD}" "$PRAXIS_DOCKER_DIR"
    echo
    printf "  %bInside the container${NC}\n" "${CYAN}${BOLD}"
    echo "    $COMPOSE_CMD exec praxis praxisctl status"
    echo "    $COMPOSE_CMD exec praxis praxisctl webserver disable"
    echo "    $COMPOSE_CMD exec praxis praxisctl set-rabbitmqurl <url>"
    echo
    printf "  %bCompose lifecycle${NC}\n" "${CYAN}${BOLD}"
    echo "    cd $PRAXIS_DOCKER_DIR"
    echo "    $COMPOSE_CMD logs -f"
    echo "    $COMPOSE_CMD down"
    echo "    $COMPOSE_CMD up -d"
    echo
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
    local options=()
    local actions=()
    if [[ "$OS_KIND" == "linux" ]]; then
        options+=("Native install   - system-wide systemd (requires RabbitMQ)")
        actions+=("native")
    fi
    options+=("Docker install   - rabbitmq + service in containers")
    actions+=("docker")
    options+=("Client only      - install only the praxis CLI")
    actions+=("client")
    options+=("Cancel")
    actions+=("cancel")

    MENU_FOOTER="${DIM}\033[3mNote: client will always be installed natively.${NC}"
    select_menu "${BOLD}Install service as${NC}" "${options[@]}"
    MENU_FOOTER=""
    local choice="${actions[$SELECTED]}"
    echo

    case "$choice" in
        cancel) error "Aborted." ;;
    esac

    get_latest_version

    case "$choice" in
        native)
            install_cli_native
            install_service_native
            print_native_summary
            ;;
        docker)
            install_cli_native
            install_service_docker
            print_docker_summary
            ;;
        client)
            install_cli_native
            echo -e "${GREEN}CLI installed.${NC} Run: praxis"
            ;;
    esac
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
