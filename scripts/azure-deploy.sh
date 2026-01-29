#!/usr/bin/env bash

#
# Azure Deployment Script for Praxis
# Deploys Docker containers to Azure Container Apps with ACR
#
# Usage:
#   ./azure-deploy.sh          Deploy Praxis to Azure
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
LOCATION="${AZURE_LOCATION:-eastus}"
ACR_NAME="${AZURE_ACR_NAME:-praxisacr}"
CONTAINER_APP_ENV="${AZURE_CONTAINER_APP_ENV:-praxis-env}"
STORAGE_ACCOUNT="${AZURE_STORAGE_ACCOUNT:-praxisstorage}"
RABBITMQ_FILE_SHARE="rabbitmq-data"
PRAXIS_FILE_SHARE="praxis-data"

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

create_acr() {
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
    info "Creating storage account for persistent data..."

    #
    # Generate unique storage account name using subscription ID hash.
    #
    SUBSCRIPTION_ID=$(az account show --query id --output tsv)
    HASH_SUFFIX=$(echo -n "$SUBSCRIPTION_ID" | md5sum | cut -c1-8)
    STORAGE_ACCOUNT_LOWER=$(echo "${STORAGE_ACCOUNT}${HASH_SUFFIX}" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]' | cut -c1-24)

    if az storage account show --name "$STORAGE_ACCOUNT_LOWER" --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "Storage account already exists"
    else
        info "Creating storage account: $STORAGE_ACCOUNT_LOWER"
        az storage account create \
            --resource-group "$RESOURCE_GROUP" \
            --name "$STORAGE_ACCOUNT_LOWER" \
            --location "$LOCATION" \
            --sku Standard_LRS \
            --kind StorageV2 \
            --output none || {
                error "Failed to create storage account. The name '$STORAGE_ACCOUNT_LOWER' might be taken."
            }
        success "Created storage account"
    fi

    STORAGE_KEY=$(az storage account keys list \
        --resource-group "$RESOURCE_GROUP" \
        --account-name "$STORAGE_ACCOUNT_LOWER" \
        --query '[0].value' \
        --output tsv)

    info "Creating file share for RabbitMQ..."
    if az storage share show \
        --name "$RABBITMQ_FILE_SHARE" \
        --account-name "$STORAGE_ACCOUNT_LOWER" \
        --account-key "$STORAGE_KEY" &> /dev/null; then
        success "File share already exists"
    else
        az storage share create \
            --name "$RABBITMQ_FILE_SHARE" \
            --account-name "$STORAGE_ACCOUNT_LOWER" \
            --account-key "$STORAGE_KEY" \
            --quota 10 \
            --output none
        success "Created file share"
    fi

    info "Creating file share for Praxis data..."
    if az storage share show \
        --name "$PRAXIS_FILE_SHARE" \
        --account-name "$STORAGE_ACCOUNT_LOWER" \
        --account-key "$STORAGE_KEY" &> /dev/null; then
        success "File share already exists"
    else
        az storage share create \
            --name "$PRAXIS_FILE_SHARE" \
            --account-name "$STORAGE_ACCOUNT_LOWER" \
            --account-key "$STORAGE_KEY" \
            --quota 5 \
            --output none
        success "Created file share"
    fi

    echo ""
}

create_container_app_environment() {
    info "Creating Container App Environment..."

    if az containerapp env show \
        --name "$CONTAINER_APP_ENV" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        success "Container App Environment already exists"
    else
        az containerapp env create \
            --name "$CONTAINER_APP_ENV" \
            --resource-group "$RESOURCE_GROUP" \
            --location "$LOCATION" \
            --output none
        success "Created Container App Environment"
    fi

    echo ""
}

deploy_rabbitmq() {
    info "Deploying RabbitMQ as Azure Container Instance..."

    #
    # RabbitMQ is deployed as an Azure Container Instance instead of Container App
    # because TCP transport in Container Apps requires custom VNET configuration.
    # ACI supports TCP natively for both internal and external access.
    #

    #
    # Get storage account details for persistent storage.
    #
    SUBSCRIPTION_ID=$(az account show --query id --output tsv)
    HASH_SUFFIX=$(echo -n "$SUBSCRIPTION_ID" | md5sum | cut -c1-8)
    STORAGE_ACCOUNT_LOWER=$(echo "${STORAGE_ACCOUNT}${HASH_SUFFIX}" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]' | cut -c1-24)

    STORAGE_KEY=$(az storage account keys list \
        --resource-group "$RESOURCE_GROUP" \
        --account-name "$STORAGE_ACCOUNT_LOWER" \
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
            --dns-name-label "praxis-rabbitmq-${LOCATION}" \
            --azure-file-volume-account-name "$STORAGE_ACCOUNT_LOWER" \
            --azure-file-volume-account-key "$STORAGE_KEY" \
            --azure-file-volume-share-name "$RABBITMQ_FILE_SHARE" \
            --azure-file-volume-mount-path /mnt/data \
            --environment-variables \
                RABBITMQ_DEFAULT_USER=praxis \
                RABBITMQ_DEFAULT_PASS=praxis \
                RABBITMQ_MNESIA_BASE=/mnt/data/mnesia \
                RABBITMQ_LOG_BASE=/mnt/data/log \
            --output none

        success "Deployed RabbitMQ as Azure Container Instance"
    fi
    echo ""
}

deploy_praxis() {
    info "Deploying Praxis Container App..."

    ACR_LOGIN_SERVER="${ACR_NAME}.azurecr.io"
    IMAGE_TAG="${ACR_LOGIN_SERVER}/praxis:latest"
    ACR_PASSWORD=$(az acr credential show \
        --name "$ACR_NAME" \
        --query 'passwords[0].value' \
        --output tsv)

    #
    # Get storage account details for persistent database storage.
    #
    SUBSCRIPTION_ID=$(az account show --query id --output tsv)
    HASH_SUFFIX=$(echo -n "$SUBSCRIPTION_ID" | md5sum | cut -c1-8)
    STORAGE_ACCOUNT_LOWER=$(echo "${STORAGE_ACCOUNT}${HASH_SUFFIX}" | tr '[:upper:]' '[:lower:]' | tr -cd '[:alnum:]' | cut -c1-24)

    STORAGE_KEY=$(az storage account keys list \
        --resource-group "$RESOURCE_GROUP" \
        --account-name "$STORAGE_ACCOUNT_LOWER" \
        --query '[0].value' \
        --output tsv)

    #
    # Configure Azure Files storage in Container App Environment.
    #
    STORAGE_NAME="praxis-db-storage"
    if az containerapp env storage show \
        --name "$STORAGE_NAME" \
        --environment-name "$CONTAINER_APP_ENV" \
        --resource-group "$RESOURCE_GROUP" &> /dev/null; then
        info "Storage already configured in environment"
    else
        info "Configuring Azure Files storage in Container App Environment..."
        az containerapp env storage set \
            --name "$CONTAINER_APP_ENV" \
            --resource-group "$RESOURCE_GROUP" \
            --storage-name "$STORAGE_NAME" \
            --storage-type AzureFile \
            --azure-file-account-name "$STORAGE_ACCOUNT_LOWER" \
            --azure-file-account-key "$STORAGE_KEY" \
            --azure-file-share-name "$PRAXIS_FILE_SHARE" \
            --access-mode ReadWrite \
            --output none
        success "Configured storage in environment"
    fi

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
                PRAXIS_DB_PATH="/app/data/.praxis_operations.db" \
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
        info "Creating Praxis Container App with persistent storage..."

        #
        # Create temporary YAML file for volume mount configuration.
        #
        TEMP_YAML=$(mktemp)
        cat > "$TEMP_YAML" <<'EOF'
properties:
  template:
    volumes:
    - name: praxis-data-volume
      storageType: AzureFile
      storageName: STORAGE_NAME_PLACEHOLDER
    containers:
    - name: PRAXIS_APP_PLACEHOLDER
      image: IMAGE_TAG_PLACEHOLDER
      resources:
        cpu: 1
        memory: 2Gi
      env:
      - name: PRAXIS_RABBITMQ_URL
        value: RABBITMQ_URL_PLACEHOLDER
      - name: PRAXIS_DB_PATH
        value: /app/data/.praxis_operations.db
      - name: RUST_LOG
        value: info
      volumeMounts:
      - volumeName: praxis-data-volume
        mountPath: /app/data
    scale:
      minReplicas: 1
      maxReplicas: 1
  configuration:
    ingress:
      external: true
      targetPort: 8080
    registries:
    - server: ACR_LOGIN_SERVER_PLACEHOLDER
      username: ACR_NAME_PLACEHOLDER
      passwordSecretRef: registry-password
    secrets:
    - name: registry-password
      value: ACR_PASSWORD_PLACEHOLDER
EOF

        #
        # Replace placeholders with actual values.
        #
        sed -i "s|STORAGE_NAME_PLACEHOLDER|$STORAGE_NAME|g" "$TEMP_YAML"
        sed -i "s|PRAXIS_APP_PLACEHOLDER|$PRAXIS_APP|g" "$TEMP_YAML"
        sed -i "s|IMAGE_TAG_PLACEHOLDER|$IMAGE_TAG|g" "$TEMP_YAML"
        sed -i "s|RABBITMQ_URL_PLACEHOLDER|$RABBITMQ_URL|g" "$TEMP_YAML"
        sed -i "s|ACR_LOGIN_SERVER_PLACEHOLDER|$ACR_LOGIN_SERVER|g" "$TEMP_YAML"
        sed -i "s|ACR_NAME_PLACEHOLDER|$ACR_NAME|g" "$TEMP_YAML"
        sed -i "s|ACR_PASSWORD_PLACEHOLDER|$ACR_PASSWORD|g" "$TEMP_YAML"

        az containerapp create \
            --name "$PRAXIS_APP" \
            --resource-group "$RESOURCE_GROUP" \
            --environment "$CONTAINER_APP_ENV" \
            --yaml "$TEMP_YAML" \
            --output none

        rm -f "$TEMP_YAML"
    fi

    success "Deployed Praxis"
    echo ""
}

print_summary() {
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

    echo -e "${GREEN}"
    echo "=============================================="
    echo "  Deployment Complete!"
    echo "=============================================="
    echo -e "${NC}"
    echo -e "${CYAN}Praxis Web UI (External HTTPS):${NC}"
    echo "  URL: https://${PRAXIS_FQDN}"
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
    echo "  ./azure-deploy.sh           Deploy Praxis to Azure"
    echo "  ./azure-deploy.sh --delete  Delete all Azure resources"
    echo "  ./azure-deploy.sh --help    Show this help message"
    echo ""
    echo "Environment Variables (optional):"
    echo "  AZURE_RESOURCE_GROUP        Resource group name (default: praxis-rg)"
    echo "  AZURE_LOCATION              Azure region (default: eastus)"
    echo "  AZURE_ACR_NAME              Container registry name (default: praxisacr)"
    echo "  AZURE_CONTAINER_APP_ENV     Container app environment (default: praxis-env)"
    echo "  AZURE_STORAGE_ACCOUNT       Storage account prefix (default: praxisstorage)"
    echo ""
    echo "Example:"
    echo "  export AZURE_RESOURCE_GROUP=\"my-praxis-rg\""
    echo "  export AZURE_LOCATION=\"westus2\""
    echo "  ./azure-deploy.sh"
    echo ""
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
    echo "  • Azure Container Registry"
    echo "  • Storage Account"
    echo "  • Log Analytics Workspace"
    echo "  • Container App Environment"
    echo "  • Resource Group"
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
    create_acr
    build_and_push_image
    push_rabbitmq_image
    create_storage
    create_container_app_environment
    deploy_rabbitmq
    deploy_praxis
    print_summary
}

main() {
    #
    # Parse command-line arguments.
    #
    case "${1:-}" in
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
