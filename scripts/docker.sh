#!/usr/bin/env bash
#
# Praxis Docker Installation Script
# Usage: curl -fsSL https://praxis.originhq.com/docker.sh | bash
#

set -e

PRAXIS_DIR="${PRAXIS_DIR:-$HOME/.praxis-docker}"
PRAXIS_REPO="originsec/praxis"
PRAXIS_VERSION="${PRAXIS_VERSION:-}"

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
    echo -e "${NC}Praxis Docker Setup"
    echo "by [Ø] Origin"
    echo ""
}

info() { echo -e "${CYAN}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

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

check_docker() {
    info "Checking prerequisites..."

    if ! command -v docker &> /dev/null; then
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
    elif command -v docker-compose &> /dev/null; then
        COMPOSE_CMD="docker-compose"
        success "Found docker-compose (standalone)"
    else
        error "Docker Compose not found. Please install Docker Compose."
    fi

    echo ""
}

setup_files() {
    local raw_url="https://raw.githubusercontent.com/$PRAXIS_REPO/$PRAXIS_VERSION"

    info "Setting up Praxis $PRAXIS_VERSION in $PRAXIS_DIR..."
    mkdir -p "$PRAXIS_DIR"
    cd "$PRAXIS_DIR"

    info "Downloading Docker configuration..."
    curl -fsSL "$raw_url/Dockerfile" -o Dockerfile
    curl -fsSL "$raw_url/docker-compose.yml" -o docker-compose.yml
    curl -fsSL "$raw_url/.dockerignore" -o .dockerignore

    #
    # Download source for build context.
    #
    info "Downloading Praxis source..."
    curl -fsSL "$raw_url/Cargo.toml" -o Cargo.toml
    curl -fsSL "$raw_url/Cargo.lock" -o Cargo.lock

    for dir in common node semantic_parser semantic_ops service web; do
        mkdir -p "$dir"
        #
        # Download directory contents via GitHub API.
        #
        curl -fsSL "https://api.github.com/repos/$PRAXIS_REPO/contents/$dir?ref=$PRAXIS_VERSION" | \
            grep '"download_url"' | \
            sed 's/.*"download_url": "\([^"]*\)".*/\1/' | \
            while read -r url; do
                if [ -n "$url" ] && [ "$url" != "null" ]; then
                    filename=$(basename "$url")
                    curl -fsSL "$url" -o "$dir/$filename" 2>/dev/null || true
                fi
            done
    done

    success "Downloaded configuration files"
    echo ""
}

clone_repo() {
    info "Setting up Praxis $PRAXIS_VERSION in $PRAXIS_DIR..."

    rm -rf "$PRAXIS_DIR"
    git clone --depth 1 --branch "$PRAXIS_VERSION" "https://github.com/$PRAXIS_REPO.git" "$PRAXIS_DIR"
    cd "$PRAXIS_DIR"

    success "Praxis $PRAXIS_VERSION ready"
    echo ""
}

start_praxis() {
    info "Building and starting Praxis (this may take a few minutes on first run)..."
    echo ""

    $COMPOSE_CMD up --build -d

    echo ""
    success "Praxis is running!"
    echo ""
}

print_summary() {
    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Praxis $PRAXIS_VERSION is ready!"
    echo "=============================================="
    echo -e "${NC}"
    echo "Web UI:              http://localhost:8080"
    echo "RabbitMQ Management: http://localhost:15672"
    echo "                     (praxis / praxis)"
    echo ""
    echo "Installation:        $PRAXIS_DIR"
    echo ""
    echo -e "${CYAN}CLI:${NC}"
    echo "  docker cp praxis-praxis-1:/app/praxis_cli ./praxis_cli"
    echo "  # or after first run, find it in the data volume:"
    echo "  # docker volume inspect praxis_praxis_data"
    echo ""
    echo -e "${CYAN}Commands:${NC}"
    echo "  cd $PRAXIS_DIR"
    echo "  $COMPOSE_CMD logs -f      # View logs"
    echo "  $COMPOSE_CMD down         # Stop Praxis"
    echo "  $COMPOSE_CMD up -d        # Start Praxis"
    echo "  $COMPOSE_CMD up --build   # Rebuild and start"
    echo ""
}

main() {
    print_banner
    check_docker
    get_latest_version
    clone_repo
    start_praxis
    print_summary
}

main "$@"
