#!/usr/bin/env bash
#
# Praxis Installation Script
# Usage: curl -fsSL https://praxis.originhq.com/install.sh | bash
#

set -e

# Configuration
PRAXIS_HOME="${PRAXIS_HOME:-$HOME/.praxis}"
PRAXIS_BIN="$PRAXIS_HOME/bin"
PRAXIS_REPO="originsec/praxis"
PRAXIS_VERSION="${PRAXIS_VERSION:-}"
NODE_PLATFORM=""
PATH_UPDATED=0
NODE_SUBDIR=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

print_banner() {
    echo -e "${CYAN}"
    echo "    ____                  _     "
    echo "   / __ \_________ __  __(_)____"
    echo "  / /_/ / ___/ __ \`/ |/_/ / ___/"
    echo " / ____/ /  / /_/ />  </ (__  ) "
    echo "/_/   /_/   \__,_/_/|_/_/____/  "
    echo ""
    echo -e "${NC}Praxis Installation Script"
    echo "by [Ø] Origin"
    echo ""
}

info() { echo -e "${CYAN}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

has_cmd() { command -v "$1" &> /dev/null; }

detect_node_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            NODE_PLATFORM="linux"
            NODE_SUBDIR="linux"
            ;;
        Darwin)
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
        *)
            NODE_PLATFORM="linux"
            NODE_SUBDIR="linux"
            warn "Unknown OS '$os' - defaulting node platform to Linux."
            ;;
    esac
}

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

check_prerequisites() {
    info "Checking prerequisites..."

    # Check for git
    has_cmd git || error "git not found. Please install git."
    success "Found git"

    # Check for Rust/Cargo
    if has_cmd cargo; then
        RUST_VERSION=$(rustc --version 2>/dev/null | cut -d' ' -f2)
        success "Found Rust $RUST_VERSION"
    else
        warn "Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
        has_cmd cargo || error "Failed to install Rust"
        success "Rust installed"
    fi

    # Check Rust version (need 1.85+ for edition 2024)
    RUST_MAJOR=$(rustc --version | sed 's/rustc \([0-9]*\)\.\([0-9]*\).*/\1/')
    RUST_MINOR=$(rustc --version | sed 's/rustc \([0-9]*\)\.\([0-9]*\).*/\2/')
    if [[ "$RUST_MAJOR" -lt 1 ]] || [[ "$RUST_MAJOR" -eq 1 && "$RUST_MINOR" -lt 85 ]]; then
        warn "Rust 1.85+ required. Updating..."
        rustup update stable
    fi

    # Check for Node.js (for frontend build)
    if has_cmd node && has_cmd npm; then
        success "Found Node.js $(node --version)"
    else
        warn "Node.js not found. Frontend build may fail."
        warn "Install Node.js 18+ for the web UI."
    fi

    # Check for Docker (needed for cross-compilation)
    if has_cmd docker && docker info &>/dev/null; then
        HAS_DOCKER=true
        success "Found Docker (Windows cross-compilation available)"
    else
        HAS_DOCKER=false
        warn "Docker not running. Windows node build will be skipped."
        warn "Start Docker for Windows cross-compilation."
    fi

    # Install cross if Docker is available
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
    local shell_name
    shell_name="$(basename "${SHELL:-/bin/sh}")"
    local os
    os="$(uname -s)"

    case "$shell_name" in
        zsh)
            if [[ "$os" == "Darwin" ]]; then
                echo "$HOME/.zprofile"
            else
                echo "$HOME/.zshrc"
            fi
            ;;
        bash)
            if [[ "$os" == "Darwin" ]] && [[ -f "$HOME/.bash_profile" ]]; then
                echo "$HOME/.bash_profile"
            else
                echo "$HOME/.bashrc"
            fi
            ;;
        fish)
            echo "$HOME/.config/fish/config.fish"
            ;;
        *)
            echo "$HOME/.profile"
            ;;
    esac
}

update_shell_path() {
    local shell_rc
    shell_rc="$(detect_shell_rc)"
    local shell_name
    shell_name="$(basename "${SHELL:-/bin/sh}")"

    local path_line
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

install_praxis() {
    info "Creating directories..."
    mkdir -p "$PRAXIS_BIN"
    mkdir -p "$PRAXIS_BIN/nodes"

    detect_node_platform
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

install_services() {
    info "Installing systemd user services..."

    local systemd_dir="$HOME/.config/systemd/user"
    local env_dir="$HOME/.config/praxis"
    mkdir -p "$systemd_dir"
    mkdir -p "$env_dir"

    #
    # Environment file with defaults if it doesn't already exist.
    #

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

print_summary() {
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
    echo -e "${CYAN}Usage:${NC}"
    echo "  systemctl --user start praxis              # Start Praxis"
    echo "  systemctl --user stop praxis               # Stop Praxis"
    echo "  systemctl --user status praxis             # Check status"
    echo "  journalctl --user -u praxis-service     # Service logs"
    echo "  journalctl --user -u praxis-web         # Web logs"
    echo ""
    echo "  Praxis starts automatically on login."
    echo "  Edit ~/.config/praxis/env to configure RabbitMQ URL."
    echo ""
    echo "Web UI: http://localhost:8080"
    echo ""

    #
    # Check if RabbitMQ is reachable by parsing the configured URL.
    #

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

remove_praxis() {
    info "Removing Praxis..."

    #
    # Stop and disable systemd services.
    #

    if systemctl --user is-active praxis.service &>/dev/null; then
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

    systemctl --user daemon-reload 2>/dev/null || true
    success "Removed systemd services"

    #
    # Remove binaries.
    #

    if [[ -d "$PRAXIS_HOME" ]]; then
        rm -rf "$PRAXIS_HOME"
        success "Removed $PRAXIS_HOME"
    fi

    #
    # Remove config.
    #

    local env_dir="$HOME/.config/praxis"
    if [[ -d "$env_dir" ]]; then
        rm -rf "$env_dir"
        success "Removed $env_dir"
    fi

    #
    # Remove PATH entry from shell rc.
    #

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

main() {
    print_banner

    if [[ "${1:-}" == "--remove" ]]; then
        remove_praxis
        exit 0
    fi

    get_latest_version
    check_prerequisites
    install_praxis
    install_services
    update_shell_path
    print_summary
}

main "$@"
