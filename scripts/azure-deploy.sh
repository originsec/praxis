#!/usr/bin/env bash

#
# Azure Deployment Script for Praxis
# Deploys Docker containers to Azure Container Apps with ACR and PostgreSQL
#
# Usage:
#   ./azure-deploy.sh          Deploy Praxis to Azure with PostgreSQL
#   ./azure-deploy.sh --stop   Stop all resources (pause billing)
#   ./azure-deploy.sh --start  Start all resources
#   ./azure-deploy.sh --delete Delete all Azure resources
#   ./azure-deploy.sh --help   Show help message
#

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

#
# Configuration - Update these for your environment.
#
RESOURCE_GROUP="${AZURE_RESOURCE_GROUP:-praxis-rg}"
LOCATION="${AZURE_LOCATION:-francecentral}"
ACR_NAME="${AZURE_ACR_NAME:-praxisacr}"
CONTAINER_APP_ENV="${AZURE_CONTAINER_APP_ENV:-praxis-env}"
STORAGE_ACCOUNT="${AZURE_STORAGE_ACCOUNT:-praxisstorage}"
RABBITMQ_FILE_SHARE="rabbitmq-data"

#
# PostgreSQL Configuration
#
POSTGRES_SERVER="${AZURE_POSTGRES_SERVER:-praxis-postgres}"
POSTGRES_ADMIN="praxisadmin"
POSTGRES_PASSWORD="${PRAXIS_POSTGRES_PASS:-Praxis_db_2024!}"
POSTGRES_DB="praxis"

#
# Container App Names
#
RABBITMQ_APP="praxis-rabbitmq"
PRAXIS_APP="praxis-app"

info() { echo -e "${CYAN}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

print_banner() {
    echo -e "${CYAN}"
    echo "======================================"
    echo "  Praxis Azure Deployment"
    echo "  (PostgreSQL Backend)"
    echo "======================================"
    echo -e "${NC}"
}

check_prerequisites() {
    info "Checking prerequisites..."

    if ! command -v az &> /dev/null; then
        error "Azure CLI not found. Install from: https://docs.microsoft.com/en-us/cli/azure/install-azure-cli"
    fi
    success "Found Azure CLI"

    if ! command -v docker &> /dev/null; then
        error "Docker not found. Install from: https://docs.docker.com/get-docker/"
    fi
    success "Found Docker"

    if ! az account show &> /dev/null; then
        error "Not logged into Azure. Run: az login"
    fi
    success "Logged into Azure"

    echo ""
}

create_resource_group() {
    info "Creating resource group: $RESOURCE_GROUP..."

    if az group show --name "$RESOURCE_GROUP" &> /dev/null; then
        success "Resource group already exists"
    else
        az group create \
            --name "$RESOURCE_GROUP" \
            --location "$LOCATION" \
            --output none
        success "Created resource group"
    fi
    echo ""
}

compute_unique_names() {
    #
    # Compute all unique resource names upfront. This allows parallel deployment
    # since all functions can reference these variables.
    #
    SUBSCRIPTION_ID=$(az account show --query id --output tsv)
    HASH_SUFFIX=$(echo -n "${SUBSCRIPTION_ID}-${RESOURCE_GROUP}" | md5sum | cut -c1-8)

    ACR_NAME_UNIQUE=$(echo "${ACR_NAME}${HASH_SUFFIX}" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]' | cut -c1-50)
    ACR_NAME="$ACR_NAME_UNIQUE"

    STORAGE_ACCOUNT_UNIQUE=$(echo "${STORAGE_ACCOUNT}${HASH_SUFFIX}" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]' | cut -c1-24)

    POSTGRES_SERVER_UNIQUE=$(echo "${POSTGRES_SERVER}-${HASH_SUFFIX}" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]-' | cut -c1-63)
}

create_acr() {
    #
    # ACR name computed by compute_unique_names().
    #
    info "Creating Azure Container Registry: $ACR_NAME..."

    if az acr show --name "$ACR_NAME" --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "ACR already exists"
    else
        az acr create \
            --resource-group "$RESOURCE_GROUP" \
            --name "$ACR_NAME" \
            --sku Basic \
            --admin-enabled true \
            --output none
        success "Created ACR"
    fi
    echo ""
}

build_and_push_image() {
    info "Building and pushing Praxis image..."

    ACR_LOGIN_SERVER="${ACR_NAME}.azurecr.io"
    IMAGE_TAG="${ACR_LOGIN_SERVER}/praxis:latest"

    az acr login --name "$ACR_NAME"

    info "Building Docker image (this may take several minutes)..."
    docker build -t "$IMAGE_TAG" .
    success "Built image: $IMAGE_TAG"

    info "Pushing image to ACR..."
    docker push "$IMAGE_TAG"
    success "Pushed image to ACR"

    echo ""
}

push_rabbitmq_image() {
    info "Pulling and pushing RabbitMQ image to ACR..."

    ACR_LOGIN_SERVER="${ACR_NAME}.azurecr.io"
    RABBITMQ_PUBLIC_IMAGE="rabbitmq:3-management"
    RABBITMQ_ACR_IMAGE="${ACR_LOGIN_SERVER}/rabbitmq:3-management"

    az acr login --name "$ACR_NAME"

    info "Pulling RabbitMQ image from Docker Hub..."
    docker pull "$RABBITMQ_PUBLIC_IMAGE"
    success "Pulled RabbitMQ image"

    info "Tagging and pushing RabbitMQ image to ACR..."
    docker tag "$RABBITMQ_PUBLIC_IMAGE" "$RABBITMQ_ACR_IMAGE"
    docker push "$RABBITMQ_ACR_IMAGE"
    success "Pushed RabbitMQ image to ACR"

    echo ""
}

create_storage() {
    #
    # Storage account name computed by compute_unique_names().
    #
    info "Creating storage account for RabbitMQ persistent data..."

    if az storage account show --name "$STORAGE_ACCOUNT_UNIQUE" --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "Storage account already exists"
    else
        info "Creating storage account: $STORAGE_ACCOUNT_UNIQUE"
        az storage account create \
            --resource-group "$RESOURCE_GROUP" \
            --name "$STORAGE_ACCOUNT_UNIQUE" \
            --location "$LOCATION" \
            --sku Standard_LRS \
            --kind StorageV2 \
            --output none || {
                error "Failed to create storage account. The name '$STORAGE_ACCOUNT_UNIQUE' might be taken."
            }
        success "Created storage account"
    fi

    STORAGE_KEY=$(az storage account keys list \
        --resource-group "$RESOURCE_GROUP" \
        --account-name "$STORAGE_ACCOUNT_UNIQUE" \
        --query '[0].value' \
        --output tsv)

    info "Creating file share for RabbitMQ..."
    if az storage share show \
        --name "$RABBITMQ_FILE_SHARE" \
        --account-name "$STORAGE_ACCOUNT_UNIQUE" \
        --account-key "$STORAGE_KEY" &> /dev/null; then
        success "File share already exists"
    else
        az storage share create \
            --name "$RABBITMQ_FILE_SHARE" \
            --account-name "$STORAGE_ACCOUNT_UNIQUE" \
            --account-key "$STORAGE_KEY" \
            --quota 10 \
            --output none
        success "Created file share"
    fi

    echo ""
}

start_postgres_creation() {
    #
    # Start PostgreSQL creation in background for parallel deployment.
    #
    if az postgres flexible-server show \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "PostgreSQL server already exists"
        POSTGRES_CREATION_STARTED=false
    else
        info "Starting PostgreSQL Flexible Server creation: $POSTGRES_SERVER_UNIQUE"
        az postgres flexible-server create \
            --resource-group "$RESOURCE_GROUP" \
            --name "$POSTGRES_SERVER_UNIQUE" \
            --location "$LOCATION" \
            --admin-user "$POSTGRES_ADMIN" \
            --admin-password "$POSTGRES_PASSWORD" \
            --sku-name Standard_B1ms \
            --tier Burstable \
            --storage-size 32 \
            --version 16 \
            --public-access 0.0.0.0 \
            --output none &
        POSTGRES_CREATION_STARTED=true
        success "PostgreSQL creation initiated"
    fi
}

wait_for_postgres() {
    #
    # Wait for PostgreSQL to be ready and configure it.
    #
    info "Waiting for PostgreSQL server to be ready..."
    while true; do
        STATE=$(az postgres flexible-server show \
            --name "$POSTGRES_SERVER_UNIQUE" \
            --resource-group "$RESOURCE_GROUP" \
            --query 'state' \
            --output tsv 2>/dev/null || echo "Creating")
        if [ "$STATE" = "Ready" ]; then
            break
        elif [ "$STATE" = "Failed" ]; then
            error "PostgreSQL server creation failed"
        fi
        info "PostgreSQL state: $STATE - waiting..."
        sleep 10
    done
    success "PostgreSQL server is ready"

    #
    # Always ensure database exists.
    #
    if az postgres flexible-server db show \
        --resource-group "$RESOURCE_GROUP" \
        --server-name "$POSTGRES_SERVER_UNIQUE" \
        --database-name "$POSTGRES_DB" &> /dev/null; then
        success "Database '$POSTGRES_DB' already exists"
    else
        info "Creating database: $POSTGRES_DB"
        az postgres flexible-server db create \
            --resource-group "$RESOURCE_GROUP" \
            --server-name "$POSTGRES_SERVER_UNIQUE" \
            --database-name "$POSTGRES_DB" \
            --output none
        success "Created database"
    fi

    #
    # Always ensure firewall rule exists.
    #
    if az postgres flexible-server firewall-rule show \
        --resource-group "$RESOURCE_GROUP" \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --rule-name AllowAzureServices &> /dev/null; then
        success "Firewall rule already exists"
    else
        info "Configuring firewall rules..."
        az postgres flexible-server firewall-rule create \
            --resource-group "$RESOURCE_GROUP" \
            --name "$POSTGRES_SERVER_UNIQUE" \
            --rule-name AllowAzureServices \
            --start-ip-address 0.0.0.0 \
            --end-ip-address 0.0.0.0 \
            --output none
        success "Configured firewall"
    fi

    POSTGRES_HOST=$(az postgres flexible-server show \
        --resource-group "$RESOURCE_GROUP" \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --query 'fullyQualifiedDomainName' \
        --output tsv)

    success "PostgreSQL ready: $POSTGRES_HOST"
    echo ""
}

start_container_app_environment() {
    #
    # Start Container App Environment creation with --no-wait.
    #
    if az containerapp env show \
        --name "$CONTAINER_APP_ENV" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "Container App Environment already exists"
        ENV_CREATION_STARTED=false
    else
        info "Starting Container App Environment creation..."
        az containerapp env create \
            --name "$CONTAINER_APP_ENV" \
            --resource-group "$RESOURCE_GROUP" \
            --location "$LOCATION" \
            --no-wait \
            --output none
        ENV_CREATION_STARTED=true
        success "Container App Environment creation initiated"
    fi
}

wait_for_container_app_environment() {
    #
    # Always wait for environment to be ready, regardless of whether we started it.
    #
    info "Waiting for Container App Environment to be ready..."
    while true; do
        STATE=$(az containerapp env show \
            --name "$CONTAINER_APP_ENV" \
            --resource-group "$RESOURCE_GROUP" \
            --query 'properties.provisioningState' \
            --output tsv 2>/dev/null || echo "Creating")
        if [ "$STATE" = "Succeeded" ]; then
            break
        elif [ "$STATE" = "Failed" ]; then
            error "Container App Environment creation failed"
        fi
        info "Container App Environment state: $STATE - waiting..."
        sleep 5
    done
    success "Container App Environment ready"
}

deploy_rabbitmq() {
    #
    # Storage account name computed by compute_unique_names().
    # RabbitMQ is deployed as an Azure Container Instance instead of Container App
    # because TCP transport in Container Apps requires custom VNET configuration.
    # ACI supports TCP natively for both internal and external access.
    #
    info "Deploying RabbitMQ as Azure Container Instance..."

    STORAGE_KEY=$(az storage account keys list \
        --resource-group "$RESOURCE_GROUP" \
        --account-name "$STORAGE_ACCOUNT_UNIQUE" \
        --query '[0].value' \
        --output tsv)

    if az container show \
        --name "$RABBITMQ_APP" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "RabbitMQ container already exists, skipping deployment"
    else
        info "Creating RabbitMQ container..."

        #
        # Deploy RabbitMQ with persistent storage and TCP ports.
        # Use ACR image to avoid Docker Hub rate limits.
        #
        ACR_LOGIN_SERVER="${ACR_NAME}.azurecr.io"
        ACR_PASSWORD=$(az acr credential show \
            --name "$ACR_NAME" \
            --query 'passwords[0].value' \
            --output tsv)

        #
        # Mount Azure File Share to /mnt/data instead of /var/lib/rabbitmq to avoid
        # permission issues with .erlang.cookie file. Configure RabbitMQ to use
        # /mnt/data for persistent data while keeping cookie in container filesystem.
        #
        az container create \
            --name "$RABBITMQ_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --location "$LOCATION" \
            --image "${ACR_LOGIN_SERVER}/rabbitmq:3-management" \
            --registry-login-server "$ACR_LOGIN_SERVER" \
            --registry-username "$ACR_NAME" \
            --registry-password "$ACR_PASSWORD" \
            --os-type Linux \
            --cpu 1 \
            --memory 2 \
            --ports 5672 15672 \
            --protocol TCP \
            --ip-address Public \
            --dns-name-label "praxis-rabbitmq-${HASH_SUFFIX}" \
            --azure-file-volume-account-name "$STORAGE_ACCOUNT_UNIQUE" \
            --azure-file-volume-account-key "$STORAGE_KEY" \
            --azure-file-volume-share-name "$RABBITMQ_FILE_SHARE" \
            --azure-file-volume-mount-path /mnt/data \
            --environment-variables \
                RABBITMQ_DEFAULT_USER=praxis \
                RABBITMQ_DEFAULT_PASS=praxis \
                RABBITMQ_MNESIA_BASE=/mnt/data/mnesia \
                RABBITMQ_LOG_BASE=/mnt/data/log \
            || error "Failed to create RabbitMQ container"

        success "Deployed RabbitMQ as Azure Container Instance"
    fi
    echo ""
}

deploy_praxis() {
    #
    # Resource names computed by compute_unique_names().
    #
    info "Deploying Praxis Container App with PostgreSQL..."

    ACR_LOGIN_SERVER="${ACR_NAME}.azurecr.io"
    IMAGE_TAG="${ACR_LOGIN_SERVER}/praxis:latest"
    ACR_PASSWORD=$(az acr credential show \
        --name "$ACR_NAME" \
        --query 'passwords[0].value' \
        --output tsv)

    POSTGRES_HOST=$(az postgres flexible-server show \
        --resource-group "$RESOURCE_GROUP" \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --query 'fullyQualifiedDomainName' \
        --output tsv)

    POSTGRES_URL="postgresql://${POSTGRES_ADMIN}:${POSTGRES_PASSWORD}@${POSTGRES_HOST}:5432/${POSTGRES_DB}?sslmode=require"

    #
    # Get RabbitMQ FQDN from Azure Container Instance.
    #
    RABBITMQ_FQDN=$(az container show \
        --name "$RABBITMQ_APP" \
        --resource-group "$RESOURCE_GROUP" \
        --query 'ipAddress.fqdn' \
        --output tsv)

    RABBITMQ_URL="amqp://praxis:praxis@${RABBITMQ_FQDN}:5672"

    if az containerapp show \
        --name "$PRAXIS_APP" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        info "Updating existing Praxis app..."
        az containerapp update \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --image "$IMAGE_TAG" \
            --set-env-vars \
                PRAXIS_RABBITMQ_URL="$RABBITMQ_URL" \
                PRAXIS_DATABASE_URL="$POSTGRES_URL" \
                RUST_LOG=info \
            --output none

        #
        # Restart to pick up new image.
        #
        info "Restarting Praxis app to pick up changes..."
        REVISION=$(az containerapp revision list \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --query '[0].name' \
            --output tsv)
        az containerapp revision restart \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --revision "$REVISION" \
            --output none
    else
        info "Creating Praxis Container App..."

        az containerapp create \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --environment "$CONTAINER_APP_ENV" \
            --image "$IMAGE_TAG" \
            --registry-server "$ACR_LOGIN_SERVER" \
            --registry-username "$ACR_NAME" \
            --registry-password "$ACR_PASSWORD" \
            --target-port 8080 \
            --ingress external \
            --cpu 1 \
            --memory 2Gi \
            --min-replicas 1 \
            --max-replicas 1 \
            --env-vars \
                PRAXIS_RABBITMQ_URL="$RABBITMQ_URL" \
                PRAXIS_DATABASE_URL="$POSTGRES_URL" \
                RUST_LOG=info \
            --output none

        success "Created Praxis Container App"
    fi

    success "Deployed Praxis"
    echo ""
}

print_summary() {
    #
    # Resource names from compute_unique_names().
    #
    PRAXIS_FQDN=$(az containerapp show \
        --name "$PRAXIS_APP" \
        --resource-group "$RESOURCE_GROUP" \
        --query 'properties.configuration.ingress.fqdn' \
        --output tsv)

    RABBITMQ_FQDN=$(az container show \
        --name "$RABBITMQ_APP" \
        --resource-group "$RESOURCE_GROUP" \
        --query 'ipAddress.fqdn' \
        --output tsv 2>/dev/null || echo "Not deployed")

    RABBITMQ_IP=$(az container show \
        --name "$RABBITMQ_APP" \
        --resource-group "$RESOURCE_GROUP" \
        --query 'ipAddress.ip' \
        --output tsv 2>/dev/null || echo "N/A")

    POSTGRES_HOST=$(az postgres flexible-server show \
        --resource-group "$RESOURCE_GROUP" \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --query 'fullyQualifiedDomainName' \
        --output tsv 2>/dev/null || echo "Not deployed")

    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Deployment Complete!"
    echo "=============================================="
    echo -e "${NC}"
    echo -e "${CYAN}Praxis Web UI (External HTTPS):${NC}"
    echo "  URL: https://${PRAXIS_FQDN}"
    echo ""
    echo -e "${CYAN}PostgreSQL Database:${NC}"
    echo "  Host: ${POSTGRES_HOST}"
    echo "  Database: ${POSTGRES_DB}"
    echo "  User: ${POSTGRES_ADMIN}"
    echo "  Port: 5432 (SSL required)"
    echo ""
    echo -e "${CYAN}RabbitMQ (Direct Access):${NC}"
    echo "  Host: ${RABBITMQ_FQDN}"
    echo "  IP: ${RABBITMQ_IP}"
    echo "  AMQP Port: 5672"
    echo "  Management Port: 15672"
    echo "  Connection: amqp://praxis:praxis@${RABBITMQ_FQDN}:5672"
    echo "  Management UI: http://${RABBITMQ_FQDN}:15672 (user: praxis, pass: praxis)"
    echo ""
    echo "Resource Group: $RESOURCE_GROUP"
    echo "Location: $LOCATION"
    echo "ACR: ${ACR_NAME}.azurecr.io"
    echo ""
    echo -e "${CYAN}Management Commands:${NC}"
    echo "  az containerapp logs show -n $PRAXIS_APP -g $RESOURCE_GROUP --follow"
    echo "  az containerapp browse -n $PRAXIS_APP -g $RESOURCE_GROUP"
    echo "  az container logs --name $RABBITMQ_APP -g $RESOURCE_GROUP --follow"
    echo ""
}

show_help() {
    echo -e "${CYAN}"
    echo "======================================"
    echo "  Praxis Azure Deployment Script"
    echo "======================================"
    echo -e "${NC}"
    echo "Usage:"
    echo "  ./azure-deploy.sh           Deploy Praxis to Azure with PostgreSQL"
    echo "  ./azure-deploy.sh --stop    Stop all resources (pause billing)"
    echo "  ./azure-deploy.sh --start   Start all resources"
    echo "  ./azure-deploy.sh --delete  Delete all Azure resources"
    echo "  ./azure-deploy.sh --help    Show this help message"
    echo ""
    echo "Environment Variables (optional):"
    echo "  AZURE_RESOURCE_GROUP        Resource group name (default: praxis-rg)"
    echo "  AZURE_LOCATION              Azure region (default: francecentral)"
    echo "  AZURE_ACR_NAME              Container registry name (default: praxisacr)"
    echo "  AZURE_CONTAINER_APP_ENV     Container app environment (default: praxis-env)"
    echo "  AZURE_STORAGE_ACCOUNT       Storage account prefix (default: praxisstorage)"
    echo "  AZURE_POSTGRES_SERVER       PostgreSQL server name (default: praxis-postgres)"
    echo "  PRAXIS_POSTGRES_PASS        PostgreSQL admin password (default: Praxis_db_2024!)"
    echo ""
    echo "Example:"
    echo "  export AZURE_RESOURCE_GROUP=\"my-praxis-rg\""
    echo "  export AZURE_LOCATION=\"westus2\""
    echo "  export PRAXIS_POSTGRES_PASS=\"MySecureP@ssword123\""
    echo "  ./azure-deploy.sh"
    echo ""
}

stop_resources() {
    echo -e "${CYAN}"
    echo "======================================"
    echo "  Stopping Praxis Azure Resources"
    echo "======================================"
    echo -e "${NC}"

    #
    # Check if resource group exists.
    #
    if ! az group show --name "$RESOURCE_GROUP" &> /dev/null; then
        error "Resource group '$RESOURCE_GROUP' does not exist"
    fi

    compute_unique_names

    #
    # Stop PostgreSQL Flexible Server.
    #
    info "Stopping PostgreSQL server..."
    if az postgres flexible-server show \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        az postgres flexible-server stop \
            --resource-group "$RESOURCE_GROUP" \
            --name "$POSTGRES_SERVER_UNIQUE" \
            --output none 2>/dev/null || warn "PostgreSQL already stopped or stop failed"
        success "PostgreSQL server stopped"
    else
        warn "PostgreSQL server not found"
    fi

    #
    # Stop RabbitMQ Container Instance.
    #
    info "Stopping RabbitMQ container..."
    if az container show \
        --name "$RABBITMQ_APP" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        az container stop \
            --name "$RABBITMQ_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --output none 2>/dev/null || warn "RabbitMQ already stopped or stop failed"
        success "RabbitMQ container stopped"
    else
        warn "RabbitMQ container not found"
    fi

    #
    # Scale down Praxis Container App to 0 replicas.
    #
    info "Scaling down Praxis app to 0 replicas..."
    if az containerapp show \
        --name "$PRAXIS_APP" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        az containerapp update \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --min-replicas 0 \
            --max-replicas 0 \
            --output none 2>/dev/null || warn "Praxis app scale down failed"
        success "Praxis app scaled to 0"
    else
        warn "Praxis app not found"
    fi

    echo ""
    success "All resources stopped. Billing paused for compute resources."
    echo ""
    echo "Note: Storage accounts and Container Registry still incur minimal charges."
    echo "Use './azure-deploy.sh --start' to restart resources."
    echo ""
}

start_resources() {
    echo -e "${CYAN}"
    echo "======================================"
    echo "  Starting Praxis Azure Resources"
    echo "======================================"
    echo -e "${NC}"

    #
    # Check if resource group exists.
    #
    if ! az group show --name "$RESOURCE_GROUP" &> /dev/null; then
        error "Resource group '$RESOURCE_GROUP' does not exist"
    fi

    compute_unique_names

    #
    # Start PostgreSQL Flexible Server first (dependency for Praxis).
    #
    info "Starting PostgreSQL server (this may take a few minutes)..."
    if az postgres flexible-server show \
        --name "$POSTGRES_SERVER_UNIQUE" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        az postgres flexible-server start \
            --resource-group "$RESOURCE_GROUP" \
            --name "$POSTGRES_SERVER_UNIQUE" \
            --output none 2>/dev/null || warn "PostgreSQL already running or start failed"
        success "PostgreSQL server started"
    else
        warn "PostgreSQL server not found"
    fi

    #
    # Start RabbitMQ Container Instance.
    #
    info "Starting RabbitMQ container..."
    if az container show \
        --name "$RABBITMQ_APP" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        az container start \
            --name "$RABBITMQ_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --output none 2>/dev/null || warn "RabbitMQ already running or start failed"
        success "RabbitMQ container started"
    else
        warn "RabbitMQ container not found"
    fi

    #
    # Scale up Praxis Container App to 1 replica.
    #
    info "Scaling up Praxis app to 1 replica..."
    if az containerapp show \
        --name "$PRAXIS_APP" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        az containerapp update \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --min-replicas 1 \
            --max-replicas 1 \
            --output none 2>/dev/null || warn "Praxis app scale up failed"
        success "Praxis app scaled to 1"
    else
        warn "Praxis app not found"
    fi

    echo ""
    success "All resources started!"
    echo ""

    #
    # Print connection info.
    #
    print_summary
}

cleanup() {
    echo -e "${CYAN}"
    echo "======================================"
    echo "  Praxis Azure Cleanup"
    echo "======================================"
    echo -e "${NC}"
    echo "Resource Group: $RESOURCE_GROUP"
    echo ""

    #
    # Check if resource group exists.
    #
    if ! az group show --name "$RESOURCE_GROUP" &> /dev/null; then
        warn "Resource group '$RESOURCE_GROUP' does not exist"
        echo ""
        info "Nothing to clean up"
        exit 0
    fi

    echo "This will delete the following resources:"
    echo ""

    #
    # List all resources in the group.
    #
    info "Listing resources..."
    az resource list --resource-group "$RESOURCE_GROUP" --query "[].{Name:name, Type:type}" --output table

    echo ""
    echo -e "${YELLOW}WARNING: This action cannot be undone!${NC}"
    echo ""
    read -p "Are you sure you want to delete resource group '$RESOURCE_GROUP'? (yes/no): " -r
    echo ""

    if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
        info "Cleanup cancelled"
        exit 0
    fi

    info "Deleting Container Instances..."
    az container delete --name "$RABBITMQ_APP" --resource-group "$RESOURCE_GROUP" --yes 2>/dev/null || warn "rabbitmq not found or already deleted"
    success "Container Instances deleted"
    echo ""

    info "Deleting Container Apps..."
    az containerapp delete --name "$PRAXIS_APP" --resource-group "$RESOURCE_GROUP" --yes 2>/dev/null || warn "praxis-app not found or already deleted"
    success "Container Apps deleted"
    echo ""

    info "Deleting entire resource group (this may take 5-10 minutes)..."
    az group delete --name "$RESOURCE_GROUP" --yes --no-wait

    echo ""
    success "Resource group deletion initiated!"
    echo ""
    echo "The following resources are being deleted in the background:"
    echo "  - Azure Container Registry"
    echo "  - Storage Account"
    echo "  - PostgreSQL Flexible Server"
    echo "  - Log Analytics Workspace"
    echo "  - Container App Environment"
    echo "  - Resource Group"
    echo ""
    echo "To monitor deletion progress:"
    echo "  az group list --query \"[?name=='$RESOURCE_GROUP']\" -o table"
    echo ""
    echo "Deletion will complete in approximately 5-10 minutes."
    echo ""
}

deploy() {
    print_banner
    check_prerequisites
    create_resource_group
    compute_unique_names

    #
    # Phase 1: Start slow async operations (PostgreSQL, Container App Env).
    # These use --no-wait so we can do other work while they provision.
    #
    start_postgres_creation
    start_container_app_environment

    #
    # Phase 2: Fast synchronous operations while async ones run in background.
    #
    create_acr
    create_storage

    #
    # Phase 3: Build and push images (ACR is now ready).
    #
    build_and_push_image
    push_rabbitmq_image

    #
    # Phase 4: Wait for async operations to complete.
    #
    wait_for_postgres
    wait_for_container_app_environment

    #
    # Phase 5: Deploy containers (all dependencies ready).
    #
    deploy_rabbitmq
    deploy_praxis

    print_summary
}

main() {
    #
    # Parse command-line arguments.
    #
    case "${1:-}" in
        --stop|-s)
            stop_resources
            ;;
        --start)
            start_resources
            ;;
        --delete|-d)
            cleanup
            ;;
        --help|-h)
            show_help
            ;;
        "")
            deploy
            ;;
        *)
            error "Unknown argument: $1. Use --help for usage information."
            ;;
    esac
}

main "$@"
