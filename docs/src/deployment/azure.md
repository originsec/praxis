# Azure Deployment

This guide covers deploying Praxis to Azure using Azure Container Apps with automatic scaling, persistent storage, and external access.

## Architecture

```
┌──────────────────────────────────────────────────┐
│                      Azure                       │
│                                                  │
│  ┌──────────────┐    ┌──────────────────────┐   │
│  │ Container    │    │  Container Instance  │   │
│  │ App (Praxis) │◄───│  (RabbitMQ)          │   │
│  └──────┬───────┘    └──────────────────────┘   │
│         │                       │                │
│  ┌──────▼───────┐    ┌─────────▼────────┐       │
│  │    SQLite    │    │  Azure File Share│       │
│  │  (internal)  │    │  (persistence)   │       │
│  └──────────────┘    └──────────────────┘       │
│                                                  │
└──────────────────────────────────────────────────┘
            │
            │ Internet
            │
      ┌─────▼─────┐
      │   Nodes   │
      │ (Targets) │
      └───────────┘
```

## Prerequisites

1. **Azure CLI** - Install from https://docs.microsoft.com/en-us/cli/azure/install-azure-cli
2. **Docker** - Install from https://docs.docker.com/get-docker/
3. **Azure Subscription** - Active subscription with appropriate permissions

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
- Set up external access
- Display connection details

### 3. Access Your Deployment

After deployment completes, you'll receive URLs for:
- **Web Interface (HTTPS)**: `https://praxis-app.{region}.azurecontainerapps.io`
- **RabbitMQ (AMQP)**: `amqp://praxis:praxis@praxis-rabbitmq-{region}.{region}.azurecontainer.io:5672`
- **RabbitMQ Management UI**: `http://praxis-rabbitmq-{region}.{region}.azurecontainer.io:15672`

## Configuration

Customize deployment by setting environment variables before running the script:

```bash
export AZURE_RESOURCE_GROUP="praxis-rg"
export AZURE_LOCATION="eastus"
export AZURE_ACR_NAME="praxisacr"
export AZURE_CONTAINER_APP_ENV="praxis-env"
export AZURE_STORAGE_ACCOUNT="praxisstorage"

./scripts/azure-deploy.sh
```

| Variable | Default | Description |
|----------|---------|-------------|
| `AZURE_RESOURCE_GROUP` | `praxis-rg` | Resource group name |
| `AZURE_LOCATION` | `eastus` | Azure region |
| `AZURE_ACR_NAME` | `praxisacr` | Container registry name |
| `AZURE_CONTAINER_APP_ENV` | `praxis-env` | Container app environment |
| `AZURE_STORAGE_ACCOUNT` | `praxisstorage` | Storage account prefix |

## What Gets Deployed

1. **Azure Container Registry (ACR)** - Stores the Praxis Docker image
2. **Azure Storage Account** - File share for RabbitMQ persistence
3. **Container App Environment** - Managed environment for Container Apps
4. **RabbitMQ** - Azure Container Instance with persistent storage and public access
5. **Praxis** - Container App with external HTTPS ingress and auto-scaling (1-3 replicas)

### External Access

**Praxis Web Interface:**
- URL: `https://praxis-app.<region>.azurecontainerapps.io`
- Protocol: HTTPS (automatically provisioned)

**RabbitMQ:**
- AMQP: `amqp://praxis:praxis@praxis-rabbitmq-<region>.<region>.azurecontainer.io:5672`
- Management UI: `http://praxis-rabbitmq-<region>.<region>.azurecontainer.io:15672`
- Credentials: `praxis` / `praxis`

## Updating Deployments

After making code changes, redeploy by running the script again:

```bash
./scripts/azure-deploy.sh
```

The script will build a new image and update the existing deployment (typically 2-3 minutes).

To update a specific component:

```bash
# Update Praxis to specific version
az containerapp update -n praxis-app -g praxis-rg --image praxisacr.azurecr.io/praxis:0.1.0

# Update Praxis to latest
az containerapp update -n praxis-app -g praxis-rg --image praxisacr.azurecr.io/praxis:latest

# Restart RabbitMQ
az container restart --name praxis-rabbitmq -g praxis-rg
```

## Management Commands

```bash
# View Praxis logs (real-time)
az containerapp logs show -n praxis-app -g praxis-rg --follow

# View RabbitMQ logs (real-time)
az container logs --name praxis-rabbitmq -g praxis-rg --follow

# Open Praxis in browser
az containerapp browse -n praxis-app -g praxis-rg

# Scale Praxis manually
az containerapp update -n praxis-app -g praxis-rg --min-replicas 2 --max-replicas 5

# Get shell access to Praxis container
az containerapp exec -n praxis-app -g praxis-rg --command /bin/bash
```

## Troubleshooting

```bash
# Check app status
az containerapp show -n praxis-app -g praxis-rg --query properties.runningStatus

# View recent logs
az containerapp logs show -n praxis-app -g praxis-rg --tail 100
az container logs --name praxis-rabbitmq -g praxis-rg --tail 100

# Check RabbitMQ status
az container show --name praxis-rabbitmq -g praxis-rg --query instanceView.state

# Test connectivity
curl https://praxis-app.<region>.azurecontainerapps.io
nc -zv praxis-rabbitmq-<region>.<region>.azurecontainer.io 5672
```

## Cost Considerations

| Component | Estimated Cost |
|-----------|---------------|
| ACR Basic | ~$5/month |
| Storage Account | ~$2/month for 10GB |
| Container Apps (Praxis) | ~$0.000012/vCPU-second |
| Container Instance (RabbitMQ) | ~$0.0000012/vCPU-second |
| **Estimated Total** | **$30-60/month** for light usage |

## Security Best Practices

1. **Change default RabbitMQ credentials** in production
2. **Restrict public access** using Network Security Groups or Azure Firewall
3. **Use Azure Key Vault** for secrets management
4. **Enable Azure AD authentication** for management access
5. **Regular security updates** of base images

## Cleanup

Use the deployment script to remove all resources:

```bash
./scripts/azure-deploy.sh --delete
```

This deletes:
- Container Instance (RabbitMQ)
- Container App (Praxis)
- Azure Container Registry
- Storage Account
- Log Analytics Workspace
- Container App Environment

Or delete the resource group directly:

```bash
az group delete --name praxis-rg --yes --no-wait
```

Verify deletion:

```bash
az group list --query "[?name=='praxis-rg']" -o table
```
