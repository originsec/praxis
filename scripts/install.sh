#!/usr/bin/env bash
#
# Praxis Installation Script
# Usage: curl -fsSL https://praxis.originhq.com/install.sh | bash
#
# Linux and macOS only. Windows users should use install.ps1.
#
# Interactive menu lets you choose:
#   - Native install (Linux/macOS)
#   - Docker install (Linux/macOS)
#   - AUR install   (Arch Linux only)
#
# Non-interactive flags:
#   --native        Force native install
#   --docker        Force docker install
#   --aur           Force AUR install
#   --remove        Remove a previous native install
#   --help          Show usage
#

set -e

#
# Configuration shared between flows.
#

PRAXIS_REPO="originsec/praxis"
PRAXIS_VERSION="${PRAXIS_VERSION:-}"

PRAXIS_HOME="${PRAXIS_HOME:-$HOME/.praxis}"
PRAXIS_BIN="$PRAXIS_HOME/bin"
PRAXIS_DOCKER_DIR="${PRAXIS_DIR:-$HOME/.praxis-docker}"

NODE_PLATFORM=""
NODE_SUBDIR=""
PATH_UPDATED=0
HAS_DOCKER=false
COMPOSE_CMD=""
OS_KIND=""        # linux | macos
IS_ARCH=false
AUR_HELPER=""

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
    echo
    echo -e "${CYAN}${BOLD}"
    echo '   ██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗'
    echo '   ██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝'
    echo '   ██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗'
    echo '   ██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║'
    echo '   ██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║'
    echo '   ╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝'
    echo -e "${NC}${DIM}            discover · control · orchestrate${NC}"
    echo -e "                  ${MAGENTA}by [Ø] Origin${NC}"
    echo
}

usage() {
    cat <<EOF
Usage: install.sh [flag]

Flags:
  --native     Native install (Linux/macOS)
  --docker     Docker install (Linux/macOS)
  --aur        AUR install (Arch Linux)
  --remove     Remove a previous native install
  --help       Show this message

If no flag is given, an interactive menu is shown.
EOF
}

#
# Platform detection.
#

detect_platform() {
    local os arch
    os="$(uname -s 2>/dev/null || echo unknown)"
    arch="$(uname -m 2>/dev/null || echo unknown)"

    case "$os" in
        Linux)
            OS_KIND="linux"
            NODE_PLATFORM="linux"
            NODE_SUBDIR="linux"
            if [[ -f /etc/os-release ]] && grep -qiE '^ID(_LIKE)?=.*arch' /etc/os-release; then
                IS_ARCH=true
            fi
            ;;
        Darwin)
            OS_KIND="macos"
            if [[ "$arch" == "arm64" || "$arch" == "aarch64" ]]; then
                NODE_PLATFORM="macos-arm64"
                NODE_SUBDIR="macos-arm64"
            elif [[ "$arch" == "x86_64" ]]; then
                NODE_PLATFORM="macos-x86_64"
                NODE_SUBDIR="macos-x86_64"
            else
                NODE_PLATFORM="macos"
                NODE_SUBDIR="macos"
            fi
            ;;
        MINGW*|MSYS*|CYGWIN*|Windows_NT)
            error "Windows is not supported by install.sh. Use install.ps1 instead."
            ;;
        *)
            OS_KIND="linux"
            NODE_PLATFORM="linux"
            NODE_SUBDIR="linux"
            warn "Unknown OS '$os' - defaulting to Linux."
            ;;
    esac

    if has_cmd yay;  then AUR_HELPER="yay";
    elif has_cmd paru; then AUR_HELPER="paru";
    fi
}

#
# Arrow-key menu. Reads from /dev/tty so it works under `curl | bash`.
#
# Args: prompt, options...
# Result: $SELECTED is set to the chosen index.
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
        error "No TTY available. Re-run with one of: --native, --docker, --aur"
    fi

    printf '\033[?25l' > "$tty_out"
    trap 'printf "\033[?25h" > '"$tty_out"'; stty echo 2>/dev/null || true' EXIT INT TERM

    printf "%b\n" "$prompt" > "$tty_out"
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
                '[A'|'OA') (( sel = (sel - 1 + n) % n ));;
                '[B'|'OB') (( sel = (sel + 1) % n ));;
            esac
        elif [[ -z $key ]]; then
            break
        elif [[ $key == $'\n' || $key == $'\r' ]]; then
            break
        elif [[ $key =~ ^[0-9]$ ]]; then
            local idx=$((key - 1))
            if (( idx >= 0 && idx < n )); then sel=$idx; break; fi
        elif [[ $key == "k" ]]; then
            (( sel = (sel - 1 + n) % n ))
        elif [[ $key == "j" ]]; then
            (( sel = (sel + 1) % n ))
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
# Version resolution shared by all flows.
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
# === Native install flow =====================================================
#

check_prerequisites() {
    info "Checking prerequisites..."

    has_cmd git || error "git not found. Please install git."
    success "Found git"

    if has_cmd cargo; then
        RUST_VERSION=$(rustc --version 2>/dev/null | cut -d' ' -f2)
        success "Found Rust $RUST_VERSION"
    else
        warn "Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        # shellcheck disable=SC1090
        source "$HOME/.cargo/env"
        has_cmd cargo || error "Failed to install Rust"
        success "Rust installed"
    fi

    RUST_MAJOR=$(rustc --version | sed 's/rustc \([0-9]*\)\.\([0-9]*\).*/\1/')
    RUST_MINOR=$(rustc --version | sed 's/rustc \([0-9]*\)\.\([0-9]*\).*/\2/')
    if [[ "$RUST_MAJOR" -lt 1 ]] || [[ "$RUST_MAJOR" -eq 1 && "$RUST_MINOR" -lt 85 ]]; then
        warn "Rust 1.85+ required. Updating..."
        rustup update stable
    fi

    if has_cmd node && has_cmd npm; then
        success "Found Node.js $(node --version)"
    else
        warn "Node.js not found. Frontend build may fail."
        warn "Install Node.js 18+ for the web UI."
    fi

    if has_cmd docker && docker info &>/dev/null; then
        HAS_DOCKER=true
        success "Found Docker (Windows cross-compilation available)"
    else
        HAS_DOCKER=false
        warn "Docker not running. Windows node build will be skipped."
        warn "Start Docker for Windows cross-compilation."
    fi

    if [[ "$HAS_DOCKER" == true ]]; then
        CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
        CROSS_BIN="$CARGO_BIN_DIR/cross"
        if ! has_cmd cross && [[ ! -x "$CROSS_BIN" ]]; then
            info "Installing cross for Windows builds..."
            cargo install cross --git https://github.com/cross-rs/cross
            success "Installed cross"
        else
            success "Found cross"
        fi
    fi

    echo ""
}

detect_shell_rc() {
    local shell_name os
    shell_name="$(basename "${SHELL:-/bin/sh}")"
    os="$(uname -s)"

    case "$shell_name" in
        zsh)
            if [[ "$os" == "Darwin" ]]; then echo "$HOME/.zprofile"; else echo "$HOME/.zshrc"; fi ;;
        bash)
            if [[ "$os" == "Darwin" ]] && [[ -f "$HOME/.bash_profile" ]]; then
                echo "$HOME/.bash_profile"
            else
                echo "$HOME/.bashrc"
            fi ;;
        fish) echo "$HOME/.config/fish/config.fish" ;;
        *)    echo "$HOME/.profile" ;;
    esac
}

update_shell_path() {
    local shell_rc shell_name path_line
    shell_rc="$(detect_shell_rc)"
    shell_name="$(basename "${SHELL:-/bin/sh}")"

    if [[ "$shell_name" == "fish" ]]; then
        path_line="fish_add_path $PRAXIS_BIN"
    else
        path_line="export PATH=\"\$PATH:$PRAXIS_BIN\""
    fi

    mkdir -p "$(dirname "$shell_rc")"

    if [[ -f "$shell_rc" ]] && grep -Fq "$PRAXIS_BIN" "$shell_rc"; then
        success "PATH already configured in $shell_rc"
    else
        info "Adding $PRAXIS_BIN to PATH in $shell_rc"
        printf "\n# Praxis\n%s\n" "$path_line" >> "$shell_rc"
        PATH_UPDATED=1
        success "Updated $shell_rc"
    fi

    export PATH="$PATH:$PRAXIS_BIN"
}

install_praxis_native() {
    info "Creating directories..."
    mkdir -p "$PRAXIS_BIN"
    mkdir -p "$PRAXIS_BIN/nodes"
    mkdir -p "$PRAXIS_BIN/nodes/$NODE_SUBDIR"
    mkdir -p "$PRAXIS_BIN/nodes/linux"
    mkdir -p "$PRAXIS_BIN/nodes/windows"

    local repo_url="https://github.com/$PRAXIS_REPO"

    info "Installing praxis_service, praxis_web, and praxis_cli..."
    cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$PRAXIS_HOME" praxis_service praxis_web praxis_cli
    success "Installed praxis_service, praxis_web, and praxis_cli"

    local node_version_file="$PRAXIS_BIN/nodes/$NODE_SUBDIR/.praxis_node_version"
    if [[ -f "$PRAXIS_BIN/nodes/$NODE_SUBDIR/praxis_node" ]] && \
       [[ -f "$node_version_file" ]] && [[ "$(cat "$node_version_file")" == "$PRAXIS_VERSION" ]]; then
        success "praxis_node ($NODE_PLATFORM) $PRAXIS_VERSION already installed, skipping"
    else
        info "Installing praxis_node ($NODE_PLATFORM)..."
        cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$PRAXIS_HOME" praxis_node
        mv "$PRAXIS_BIN/praxis_node" "$PRAXIS_BIN/nodes/$NODE_SUBDIR/praxis_node"
        echo "$PRAXIS_VERSION" > "$node_version_file"
        success "Installed praxis_node ($NODE_PLATFORM)"
    fi

    if [[ "$HAS_DOCKER" == true ]]; then
        WINDOWS_VERSION_FILE="$PRAXIS_BIN/nodes/windows/.praxis_node_version"
        if [[ -f "$PRAXIS_BIN/nodes/windows/praxis_node.exe" ]] && \
           [[ -f "$WINDOWS_VERSION_FILE" ]] && [[ "$(cat "$WINDOWS_VERSION_FILE")" == "$PRAXIS_VERSION" ]]; then
            success "praxis_node (Windows) $PRAXIS_VERSION already installed, skipping"
        else
            info "Installing praxis_node (Windows) via cross..."

            TEMP_DIR=$(mktemp -d)
            git clone --depth 1 --branch "$PRAXIS_VERSION" "$repo_url" "$TEMP_DIR/praxis"

            pushd "$TEMP_DIR/praxis" > /dev/null
            "$CROSS_BIN" build --release --target x86_64-pc-windows-gnu -p praxis_node

            cp target/x86_64-pc-windows-gnu/release/praxis_node.exe "$PRAXIS_BIN/nodes/windows/"
            popd > /dev/null

            rm -rf "$TEMP_DIR"
            echo "$PRAXIS_VERSION" > "$WINDOWS_VERSION_FILE"
            success "Installed praxis_node (Windows)"
        fi
    fi

    echo ""
}

install_services_native() {
    if [[ "$OS_KIND" != "linux" ]]; then
        info "Skipping systemd services (not Linux)."
        echo ""
        return
    fi

    if ! has_cmd systemctl; then
        warn "systemctl not found - skipping systemd user services."
        echo ""
        return
    fi

    info "Installing systemd user services..."

    local systemd_dir="$HOME/.config/systemd/user"
    local env_dir="$HOME/.config/praxis"
    mkdir -p "$systemd_dir"
    mkdir -p "$env_dir"

    if [[ ! -f "$env_dir/env" ]]; then
        cat > "$env_dir/env" << EOF
PRAXIS_RABBITMQ_URL=amqp://guest:guest@localhost:5672
EOF
        success "Created $env_dir/env"
    else
        success "Environment file already exists at $env_dir/env"
    fi

    cat > "$systemd_dir/praxis-service.service" << EOF
[Unit]
Description=Praxis Service
PartOf=praxis.service

[Service]
Type=simple
ExecStart=$PRAXIS_BIN/praxis_service
EnvironmentFile=$env_dir/env
Restart=on-failure
RestartSec=3

[Install]
WantedBy=praxis.service
EOF

    cat > "$systemd_dir/praxis-web.service" << EOF
[Unit]
Description=Praxis Web
After=praxis-service.service
PartOf=praxis.service

[Service]
Type=simple
ExecStart=$PRAXIS_BIN/praxis_web
EnvironmentFile=$env_dir/env
Restart=on-failure
RestartSec=3

[Install]
WantedBy=praxis.service
EOF

    cat > "$systemd_dir/praxis.service" << EOF
[Unit]
Description=Praxis
Wants=praxis-service.service praxis-web.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/true

[Install]
WantedBy=default.target
EOF

    systemctl --user daemon-reload
    systemctl --user enable praxis.service
    success "Installed and enabled systemd user services"
    echo ""
}

print_native_summary() {
    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Praxis $PRAXIS_VERSION installation complete!"
    echo "=============================================="
    echo -e "${NC}"
    echo "Installed to: $PRAXIS_HOME"
    echo ""
    echo "Binaries:"
    echo "  $PRAXIS_BIN/praxis_service"
    echo "  $PRAXIS_BIN/praxis_web"
    echo "  $PRAXIS_BIN/praxis_cli"
    echo ""
    echo "Node agents:"
    echo "  $PRAXIS_BIN/nodes/$NODE_SUBDIR/praxis_node"
    if [[ "$HAS_DOCKER" == true ]]; then
        echo "  $PRAXIS_BIN/nodes/windows/praxis_node.exe"
    fi
    echo ""
    echo "Config:"
    echo "  ~/.config/praxis/env"
    echo ""
    if [[ "$PATH_UPDATED" -eq 1 ]]; then
        echo -e "${YELLOW}PATH updated. Restart your shell or run:${NC}"
        echo ""
        echo "  source $(detect_shell_rc)"
        echo ""
    fi
    if [[ "$OS_KIND" == "linux" ]] && has_cmd systemctl; then
        echo -e "${CYAN}Usage:${NC}"
        echo "  systemctl --user start praxis              # Start Praxis"
        echo "  systemctl --user stop praxis               # Stop Praxis"
        echo "  systemctl --user status praxis             # Check status"
        echo "  journalctl --user -u praxis-service        # Service logs"
        echo "  journalctl --user -u praxis-web            # Web logs"
        echo ""
        echo "  Praxis starts automatically on login."
        echo "  Edit ~/.config/praxis/env to configure RabbitMQ URL."
        echo ""
    fi
    echo "Web UI: http://localhost:8080"
    echo ""

    local rabbitmq_url=""
    local env_file="$HOME/.config/praxis/env"
    if [[ -f "$env_file" ]]; then
        rabbitmq_url=$(grep -oP 'PRAXIS_RABBITMQ_URL=\K.*' "$env_file" 2>/dev/null || true)
    fi
    rabbitmq_url="${rabbitmq_url:-amqp://guest:guest@localhost:5672}"

    local rabbitmq_host rabbitmq_port
    rabbitmq_host=$(echo "$rabbitmq_url" | sed -E 's|amqps?://(([^@]+)@)?([^:/?]+).*|\3|')
    rabbitmq_port=$(echo "$rabbitmq_url" | sed -E 's|.*:([0-9]+)(/.*)?$|\1|')
    rabbitmq_host="${rabbitmq_host:-localhost}"
    rabbitmq_port="${rabbitmq_port:-5672}"

    if (echo > /dev/tcp/"$rabbitmq_host"/"$rabbitmq_port") 2>/dev/null; then
        success "RabbitMQ is reachable at $rabbitmq_host:$rabbitmq_port"
    else
        warn "RabbitMQ does not appear to be running at $rabbitmq_host:$rabbitmq_port"
        echo "  Praxis requires RabbitMQ. Start it before launching Praxis:"
        echo ""
        echo "    sudo systemctl start rabbitmq-server"
        echo ""
    fi
}

run_native() {
    get_latest_version
    check_prerequisites
    install_praxis_native
    install_services_native
    update_shell_path
    print_native_summary
}

#
# === Docker install flow =====================================================
#

check_docker() {
    info "Checking prerequisites..."

    if ! has_cmd docker; then
        error "Docker not found. Please install Docker: https://docs.docker.com/get-docker/"
    fi
    success "Found Docker"

    if ! docker info &> /dev/null; then
        error "Docker daemon not running. Please start Docker."
    fi
    success "Docker daemon running"

    if docker compose version &> /dev/null; then
        COMPOSE_CMD="docker compose"
        success "Found Docker Compose (plugin)"
    elif has_cmd docker-compose; then
        COMPOSE_CMD="docker-compose"
        success "Found docker-compose (standalone)"
    else
        error "Docker Compose not found. Please install Docker Compose."
    fi

    has_cmd git || error "git not found. Please install git."
    success "Found git"

    echo ""
}

clone_repo_docker() {
    info "Setting up Praxis $PRAXIS_VERSION in $PRAXIS_DOCKER_DIR..."
    rm -rf "$PRAXIS_DOCKER_DIR"
    git clone --depth 1 --branch "$PRAXIS_VERSION" "https://github.com/$PRAXIS_REPO.git" "$PRAXIS_DOCKER_DIR"
    cd "$PRAXIS_DOCKER_DIR"
    success "Praxis $PRAXIS_VERSION ready"
    echo ""
}

start_praxis_docker() {
    info "Building and starting Praxis (this may take a few minutes on first run)..."
    echo ""
    $COMPOSE_CMD up --build -d
    echo ""
    success "Praxis is running!"
    echo ""
}

print_docker_summary() {
    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Praxis $PRAXIS_VERSION is ready!"
    echo "=============================================="
    echo -e "${NC}"
    echo "Web UI:              http://localhost:8080"
    echo "RabbitMQ Management: http://localhost:15672"
    echo "                     (praxis / praxis)"
    echo ""
    echo "Installation:        $PRAXIS_DOCKER_DIR"
    echo ""
    local container cp_source
    container=$($COMPOSE_CMD ps -q praxis 2>/dev/null | head -1)
    cp_source="${container:+$container:/app/praxis_cli}"
    [[ -z "$container" ]] && cp_source="<container_id>:/app/praxis_cli"

    echo -e "${CYAN}CLI:${NC}"
    echo "  docker cp $cp_source ./praxis_cli"
    echo ""
    echo -e "${CYAN}Commands:${NC}"
    echo "  cd $PRAXIS_DOCKER_DIR"
    echo "  $COMPOSE_CMD logs -f      # View logs"
    echo "  $COMPOSE_CMD down         # Stop Praxis"
    echo "  $COMPOSE_CMD up -d        # Start Praxis"
    echo "  $COMPOSE_CMD up --build   # Rebuild and start"
    echo ""
}

run_docker() {
    check_docker
    get_latest_version
    clone_repo_docker
    start_praxis_docker
    print_docker_summary
}

#
# === AUR install flow ========================================================
#

run_aur() {
    if [[ "$IS_ARCH" != true ]]; then
        error "AUR install is only supported on Arch Linux (and derivatives)."
    fi

    if [[ -n "$AUR_HELPER" ]]; then
        info "Installing praxis from the AUR using $AUR_HELPER..."
        "$AUR_HELPER" -S --needed praxis
    else
        warn "No AUR helper found (yay or paru)."
        echo "Falling back to a manual makepkg build."
        has_cmd git    || error "git is required."
        has_cmd makepkg || error "makepkg (base-devel) is required: sudo pacman -S --needed base-devel"

        local tmp
        tmp=$(mktemp -d)
        git clone https://aur.archlinux.org/praxis.git "$tmp/praxis"
        pushd "$tmp/praxis" > /dev/null
        makepkg -si --needed
        popd > /dev/null
        rm -rf "$tmp"
    fi

    echo ""
    success "Praxis installed from the AUR."
    echo ""
    echo -e "${CYAN}Usage:${NC}"
    echo "  systemctl --user enable --now praxis"
    echo "  Web UI: http://localhost:8080"
    echo ""
}

#
# === Remove ==================================================================
#

remove_praxis() {
    info "Removing Praxis (native install)..."

    if has_cmd systemctl && systemctl --user is-active praxis.service &>/dev/null; then
        info "Stopping services..."
        systemctl --user stop praxis.service
    fi

    local systemd_dir="$HOME/.config/systemd/user"
    local units=("praxis.service" "praxis-service.service" "praxis-web.service")
    for unit in "${units[@]}"; do
        if [[ -f "$systemd_dir/$unit" ]]; then
            systemctl --user disable "$unit" 2>/dev/null || true
            rm -f "$systemd_dir/$unit"
        fi
    done
    has_cmd systemctl && systemctl --user daemon-reload 2>/dev/null || true
    success "Removed systemd services"

    if [[ -d "$PRAXIS_HOME" ]]; then
        rm -rf "$PRAXIS_HOME"
        success "Removed $PRAXIS_HOME"
    fi

    local env_dir="$HOME/.config/praxis"
    if [[ -d "$env_dir" ]]; then
        rm -rf "$env_dir"
        success "Removed $env_dir"
    fi

    local shell_rc
    shell_rc="$(detect_shell_rc)"
    if [[ -f "$shell_rc" ]] && grep -Fq "$PRAXIS_BIN" "$shell_rc"; then
        local tmp
        tmp=$(mktemp)
        grep -v "$PRAXIS_BIN" "$shell_rc" | grep -v "^# Praxis$" > "$tmp"
        mv "$tmp" "$shell_rc"
        success "Removed PATH entry from $shell_rc"
    fi

    echo ""
    success "Praxis has been removed."
    echo ""
}

#
# === Menu / dispatch =========================================================
#

interactive_menu() {
    local options=()
    local actions=()

    options+=("Native install   - build & run as a user service")
    actions+=("native")
    options+=("Docker install   - run via docker compose")
    actions+=("docker")
    if [[ "$IS_ARCH" == true ]]; then
        if [[ -n "$AUR_HELPER" ]]; then
            options+=("AUR install      - $AUR_HELPER -S praxis (recommended on Arch)")
        else
            options+=("AUR install      - manual makepkg build (recommended on Arch)")
        fi
        actions+=("aur")
    fi

    options+=("Quit")
    actions+=("quit")

    local prompt
    prompt="${BOLD}Choose how to install Praxis${NC}  ${DIM}(↑/↓ arrows · Enter to select · q to quit)${NC}\n"
    select_menu "$prompt" "${options[@]}"

    local action="${actions[$SELECTED]}"
    echo
    case "$action" in
        native) run_native ;;
        docker) run_docker ;;
        aur)    run_aur ;;
        quit)   echo "Aborted."; exit 0 ;;
    esac
}

main() {
    print_banner
    detect_platform

    case "${1:-}" in
        --help|-h)   usage; exit 0 ;;
        --remove)    remove_praxis; exit 0 ;;
        --native)    run_native;    exit 0 ;;
        --docker)    run_docker;    exit 0 ;;
        --aur)       run_aur;       exit 0 ;;
        "")          interactive_menu ;;
        *)           usage; exit 1 ;;
    esac
}

main "$@"
