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
NODE_FILENAME=""
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
            NODE_FILENAME="praxis_node_linux"
            NODE_SUBDIR="linux"
            ;;
        Darwin)
            if [[ "$arch" == "arm64" || "$arch" == "aarch64" ]]; then
                NODE_PLATFORM="macos-arm64"
                NODE_FILENAME="praxis_node_macos_arm64"
                NODE_SUBDIR="macos-arm64"
            elif [[ "$arch" == "x86_64" ]]; then
                NODE_PLATFORM="macos-x86_64"
                NODE_FILENAME="praxis_node_macos_x86_64"
                NODE_SUBDIR="macos-x86_64"
            else
                NODE_PLATFORM="macos"
                NODE_FILENAME="praxis_node_macos"
                NODE_SUBDIR="macos"
            fi
            ;;
        *)
            NODE_PLATFORM="linux"
            NODE_FILENAME="praxis_node_linux"
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
        if ! has_cmd cross; then
            info "Installing cross for Windows builds..."
            cargo install cross --git https://github.com/cross-rs/cross
            success "Installed cross"
        else
            success "Found cross"
        fi
    fi

    echo ""
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

    info "Installing praxis_node ($NODE_PLATFORM)..."
    cargo install --git "$repo_url" --tag "$PRAXIS_VERSION" --root "$PRAXIS_HOME" praxis_node
    mv "$PRAXIS_BIN/praxis_node" "$PRAXIS_BIN/nodes/$NODE_SUBDIR/praxis_node"
    cp "$PRAXIS_BIN/nodes/$NODE_SUBDIR/praxis_node" "$PRAXIS_BIN/nodes/$NODE_FILENAME"
    success "Installed praxis_node ($NODE_PLATFORM)"

    if [[ "$HAS_DOCKER" == true ]]; then
        info "Installing praxis_node (Windows) via cross..."

        TEMP_DIR=$(mktemp -d)
        git clone --depth 1 --branch "$PRAXIS_VERSION" "$repo_url" "$TEMP_DIR/praxis"

        pushd "$TEMP_DIR/praxis" > /dev/null
        cross build --release --target x86_64-pc-windows-gnu -p praxis_node

        cp target/x86_64-pc-windows-gnu/release/praxis_node.exe "$PRAXIS_BIN/nodes/windows/"
        cp target/x86_64-pc-windows-gnu/release/praxis_node.exe "$PRAXIS_BIN/nodes/praxis_node_windows.exe"
        popd > /dev/null

        rm -rf "$TEMP_DIR"
        success "Installed praxis_node (Windows)"
    fi

    echo ""
}

install_runner() {
    info "Installing runner script..."

    cat > "$PRAXIS_BIN/praxis.sh" << 'RUNNER_EOF'
#!/usr/bin/env bash
#
# Praxis Runner
# Starts both praxis_service and praxis_web
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_RABBITMQ_URL="amqp://guest:guest@localhost:5672"
RABBITMQ_URL="${1:-${PRAXIS_RABBITMQ_URL:-$DEFAULT_RABBITMQ_URL}}"

export PRAXIS_RABBITMQ_URL="$RABBITMQ_URL"

SERVICE_PID=""
WEB_PID=""

cleanup() {
    echo ""
    echo "Shutting down Praxis..."
    [[ -n "$WEB_PID" ]] && kill "$WEB_PID" 2>/dev/null && wait "$WEB_PID" 2>/dev/null
    [[ -n "$SERVICE_PID" ]] && kill "$SERVICE_PID" 2>/dev/null && wait "$SERVICE_PID" 2>/dev/null
    echo "Praxis stopped."
    exit 0
}

trap cleanup EXIT INT TERM

echo "Starting Praxis..."
echo "  RabbitMQ: $RABBITMQ_URL"
echo ""

"$SCRIPT_DIR/praxis_service" &
SERVICE_PID=$!
sleep 1

if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
    echo "Error: praxis_service failed to start"
    exit 1
fi

"$SCRIPT_DIR/praxis_web" &
WEB_PID=$!

echo "Praxis running. Press Ctrl+C to stop."
echo "  Web UI: http://localhost:8080"
echo ""

wait "$SERVICE_PID" "$WEB_PID"
RUNNER_EOF

    chmod +x "$PRAXIS_BIN/praxis.sh"
    success "Installed praxis.sh runner"
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
    echo "  $PRAXIS_BIN/praxis.sh"
    echo ""
    echo "Node agents:"
    echo "  $PRAXIS_BIN/nodes/$NODE_SUBDIR/praxis_node"
    echo "  $PRAXIS_BIN/nodes/$NODE_FILENAME"
    if [[ "$HAS_DOCKER" == true ]]; then
        echo "  $PRAXIS_BIN/nodes/windows/praxis_node.exe"
        echo "  $PRAXIS_BIN/nodes/praxis_node_windows.exe"
    fi
    echo ""
    echo -e "${YELLOW}Add to your PATH:${NC}"
    echo ""
    echo "  export PATH=\"\$PATH:$PRAXIS_BIN\""
    echo ""
    echo "Add this to your ~/.bashrc or ~/.zshrc:"
    echo ""
    echo "  echo 'export PATH=\"\$PATH:$PRAXIS_BIN\"' >> ~/.bashrc"
    echo ""
    echo -e "${CYAN}Usage:${NC}"
    echo "  praxis.sh                    # Start with default RabbitMQ"
    echo "  praxis.sh amqp://host:5672   # Start with custom RabbitMQ"
    echo ""
    echo "Web UI: http://localhost:8080"
    echo ""
}

main() {
    print_banner
    get_latest_version
    check_prerequisites
    install_praxis
    install_runner
    print_summary
}

main "$@"
