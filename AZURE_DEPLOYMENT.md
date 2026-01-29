# Azure Deployment Guide for Praxis

This guide covers deploying Praxis to Azure using Azure Container Apps with automatic scaling, persistent storage, and external access.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Quick Start](#quick-start)
- [Full Deployment](#full-deployment)
- [External Access](#external-access)
- [Management](#management)
- [Cleanup](#cleanup)

## Prerequisites

1. **Azure CLI**: Install from https://docs.microsoft.com/en-us/cli/azure/install-azure-cli
2. **Docker**: Install from https://docs.docker.com/get-docker/
3. **Azure Subscription**: Active subscription with appropriate permissions

## Quick Start

### 1. Login to Azure

```bash
az login
az account set --subscription <your-subscription-id>
```

### 2. Deploy Praxis

```bash
cd /path/to/praxis
./scripts/azure-deploy.sh
```

The script will:
- Create all required Azure resources
- Build and push the Docker image
- Deploy Praxis with RabbitMQ
- Set up external access proxies
- Display connection details

### 3. Access Your Deployment

After deployment completes, you'll receive URLs for:
- **Web Interface (HTTPS)**: https://praxis-app.{region}.azurecontainerapps.io
- **RabbitMQ (AMQP)**: amqp://praxis:praxis@praxis-rabbitmq-{region}.{region}.azurecontainer.io:5672
- **RabbitMQ Management UI**: http://praxis-rabbitmq-{region}.{region}.azurecontainer.io:15672

## Full Deployment

### Configuration (Optional)

Customize deployment by setting environment variables:

```bash
# Infrastructure Configuration
export AZURE_RESOURCE_GROUP="praxis-rg"        # Resource group name (default: praxis-rg)
export AZURE_LOCATION="eastus"                  # Azure region (default: eastus)
export AZURE_ACR_NAME="praxisacr"              # Container registry name (default: praxisacr)
export AZURE_CONTAINER_APP_ENV="praxis-env"    # Container app environment (default: praxis-env)
export AZURE_STORAGE_ACCOUNT="praxisstorage"   # Storage account prefix (default: praxisstorage)

./scripts/azure-deploy.sh
```

### Deployment Script Usage

```bash
# Deploy Praxis
./scripts/azure-deploy.sh

# Show help
./scripts/azure-deploy.sh --help

# Delete all resources
./scripts/azure-deploy.sh --delete
```

### Configuration

Set environment variables before running:

```bash
export AZURE_RESOURCE_GROUP="praxis-rg"
export AZURE_LOCATION="eastus"
export AZURE_ACR_NAME="praxisacr"
export AZURE_CONTAINER_APP_ENV="praxis-env"
export AZURE_STORAGE_ACCOUNT="praxisstorage"

./scripts/azure-deploy.sh
```

### What It Does

1. Creates Azure Container Registry (ACR)
2. Builds and pushes Docker image to ACR
3. Creates Azure Storage Account with File Share for RabbitMQ persistence
4. Creates Container App Environment
5. Deploys RabbitMQ as Azure Container Instance with persistent storage and public access
6. Deploys Praxis application as Container App with external HTTPS ingress and auto-scaling (1-3 replicas)

### Persistent Storage

RabbitMQ data is stored in an Azure File Share mounted at `/var/lib/rabbitmq`, ensuring data persists across container restarts.

### External Access

**Praxis Web Interface:**
- URL: `https://praxis-app.<region>.azurecontainerapps.io`
- Protocol: HTTPS (automatically provisioned by Azure Container Apps)
- Deployment: Container App with external ingress

**RabbitMQ:**
- Host: `praxis-rabbitmq-<region>.<region>.azurecontainer.io`
- AMQP Port: 5672 (for application connections)
- Management UI Port: 15672 (for administration)
- AMQP Connection: `amqp://praxis:praxis@praxis-rabbitmq-<region>.<region>.azurecontainer.io:5672`
- Management UI: `http://praxis-rabbitmq-<region>.<region>.azurecontainer.io:15672`
  - Username: `praxis`
  - Password: `praxis`
- Deployment: Azure Container Instance with public IP and persistent storage

**Architecture Notes:**
- Praxis uses Container Apps for auto-scaling and managed HTTPS
- RabbitMQ uses Container Instance for native TCP support and direct public access
- Both services are externally accessible without proxies
- RabbitMQ data persists in Azure File Share

### Management Commands

```bash
# View Praxis logs (real-time)
az containerapp logs show -n praxis-app -g praxis-rg --follow

# View RabbitMQ logs (real-time)
az container logs --name praxis-rabbitmq -g praxis-rg --follow

# Open Praxis in browser
az containerapp browse -n praxis-app -g praxis-rg

# Scale Praxis manually
az containerapp update -n praxis-app -g praxis-rg --min-replicas 2 --max-replicas 5

# Restart RabbitMQ
az container restart --name praxis-rabbitmq -g praxis-rg

# Delete deployment
az containerapp delete -n praxis-app -g praxis-rg --yes
az container delete -n praxis-rabbitmq -g praxis-rg --yes
```

## Cost Considerations
- **ACR Basic**: ~$5/month
- **Storage Account**: ~$2/month for 10GB
- **Container Apps** (Praxis): ~$0.000012/vCPU-second + ~$0.0000013/GiB-second
- **Container Instance** (RabbitMQ): ~$0.0000012/vCPU-second + ~$0.0000002/GiB-second
- **Estimated Total**: $30-60/month for light usage

## Updating Deployments

### Quick Redeploy After Code Changes

After making code changes (like frontend fixes, backend updates, etc.), simply run:

```bash
./scripts/azure-deploy.sh
```

The script will:
- Build the new Docker image with your changes
- Push it to Azure Container Registry
- Update the existing Container App (won't create new resources)
- Automatically restart Praxis to pick up the new image

This typically takes 2-3 minutes for the build and deployment.

### Update Specific Component

```bash
# Update Praxis app to specific version (current: v0.1.0)
az containerapp update -n praxis-app -g praxis-rg --image praxisacr.azurecr.io/praxis:0.1.0

# Update Praxis app to latest
az containerapp update -n praxis-app -g praxis-rg --image praxisacr.azurecr.io/praxis:latest

# Update or restart RabbitMQ
az container restart --name praxis-rabbitmq -g praxis-rg
```

## Monitoring

```bash
# View Praxis application logs
az containerapp logs show -n praxis-app -g praxis-rg --tail 50

# View RabbitMQ logs
az container logs --name praxis-rabbitmq -g praxis-rg --tail 50

# Open Praxis in Azure Portal
az containerapp browse -n praxis-app -g praxis-rg
```

## Troubleshooting

```bash
# Check app status
az containerapp show -n praxis-app -g praxis-rg --query properties.runningStatus

# View recent logs (last 100 lines)
az containerapp logs show -n praxis-app -g praxis-rg --tail 100
az container logs --name praxis-rabbitmq -g praxis-rg --tail 100

# View logs in real-time
az containerapp logs show -n praxis-app -g praxis-rg --follow
az container logs --name praxis-rabbitmq -g praxis-rg --follow

# Check RabbitMQ status
az container show --name praxis-rabbitmq -g praxis-rg --query instanceView.state

# Get shell access to Praxis container
az containerapp exec -n praxis-app -g praxis-rg --command /bin/bash

# Test connectivity (replace region with your actual value)
curl https://praxis-app.<region>.azurecontainerapps.io
nc -zv praxis-rabbitmq-<region>.<region>.azurecontainer.io 5672
```

## Security Best Practices

1. **Change default RabbitMQ credentials** in production
2. **Restrict public access** using Network Security Groups or Azure Firewall
3. **Configure network policies** to restrict RabbitMQ access
4. **Use Azure Key Vault** for secrets management
5. **Enable Azure AD authentication** for management access
6. **Regular security updates** of base images

## Cleanup

### Complete Cleanup (Recommended)

Use the deployment script to remove all resources:

```bash
./scripts/azure-deploy.sh --delete
```

This will:
1. Prompt for confirmation
2. Delete Container Instance (RabbitMQ)
3. Delete Container App (Praxis)
4. Delete the entire resource group including:
   - Azure Container Registry
   - Storage Account
   - Log Analytics Workspace
   - Container App Environment

The script runs the deletion in the background and takes approximately 5-10 minutes to complete.

### Manual Cleanup

Alternatively, delete the resource group directly:

```bash
az group delete --name praxis-rg --yes --no-wait
```

### Verify Deletion

Check if the resource group has been deleted:

```bash
# Should return nothing when deletion is complete
az group list --query "[?name=='praxis-rg']" -o table
```

### Cost Note

Once deleted, you will no longer be charged for any Azure resources. The deletion removes:
- All compute resources (Container Apps, Container Instances)
- Storage (including RabbitMQ data)
- Networking resources
- Container images in ACR
