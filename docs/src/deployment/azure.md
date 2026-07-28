# Azure Deployment

This guide covers deploying Praxis to Azure using Azure Container Apps with PostgreSQL, automatic scaling, and persistent storage.

## Architecture

```diagram
┌─────────────────────────────────────────────────┐
│                      Azure                      │
│                                                 │
│  ┌──────────────┐    ┌──────────────────────┐   │
│  │ Container    │    │  Container Instance  │   │
│  │ App (Praxis) │◄───│  (RabbitMQ)          │   │
│  └──────┬───────┘    └──────────────────────┘   │
│         │                       │               │
│  ┌──────▼───────┐     ┌─────────▼────────┐      │
│  │  PostgreSQL  │     │  Azure File Share│      │
│  │  Flexible    │     │  (persistence)   │      │
│  └──────────────┘     └──────────────────┘      │
│                                                 │
└─────────────────────────────────────────────────┘
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
- Build and push Docker images to ACR
- Deploy PostgreSQL Flexible Server
- Deploy Praxis with RabbitMQ
- Display connection details

### 3. Access Your Deployment

After deployment completes, you'll receive URLs for:
- **RabbitMQ (AMQP)**: `amqp://praxis:praxis@praxis-rabbitmq-{hash}.{region}.azurecontainer.io:5672` — point your `praxis` TUI at this with `praxis set-rabbitmqurl <url>` or `PRAXIS_RABBITMQ_URL`
- **RabbitMQ Management UI**: `http://praxis-rabbitmq-{hash}.{region}.azurecontainer.io:15672`

## Script Commands

```bash
./scripts/azure-deploy.sh            # Deploy Praxis
./scripts/azure-deploy.sh --stop     # Stop all resources (pause billing)
./scripts/azure-deploy.sh --start    # Start all resources
./scripts/azure-deploy.sh --delete   # Delete all Azure resources
./scripts/azure-deploy.sh --help     # Show help
```

## Configuration

Customize deployment with environment variables:

```bash
export AZURE_RESOURCE_GROUP="praxis-rg"
export AZURE_LOCATION="westus2"
export PRAXIS_POSTGRES_PASS="MySecureP@ssword123"

./scripts/azure-deploy.sh
```

| Variable | Default | Description |
|----------|---------|-------------|
| `AZURE_RESOURCE_GROUP` | `praxis-rg` | Resource group name |
| `AZURE_LOCATION` | `francecentral` | Azure region |
| `AZURE_ACR_NAME` | `praxisacr` | Container registry name prefix |
| `AZURE_CONTAINER_APP_ENV` | `praxis-env` | Container app environment |
| `AZURE_STORAGE_ACCOUNT` | `praxisstorage` | Storage account prefix |
| `AZURE_POSTGRES_SERVER` | `praxis-postgres` | PostgreSQL server name prefix |
| `PRAXIS_POSTGRES_PASS` | `Praxis_db_2024!` | PostgreSQL admin password |

Resource names are automatically made unique using a hash suffix derived from your subscription and resource group.

## What Gets Deployed

1. **Azure Container Registry (ACR)** - Stores Praxis and RabbitMQ images
2. **Azure Storage Account** - File share for RabbitMQ persistence
3. **PostgreSQL Flexible Server** - Database backend (Burstable B1ms tier)
4. **Container App Environment** - Managed environment for Container Apps
5. **RabbitMQ** - Azure Container Instance with persistent storage
6. **Praxis** - Container App (no external ingress is configured by the script)

## Stopping and Starting

To pause billing when not using Praxis:

```bash
# Stop all resources
./scripts/azure-deploy.sh --stop
```

This will:
- Stop PostgreSQL Flexible Server
- Stop RabbitMQ Container Instance
- Scale Praxis Container App to 0 replicas

To resume:

```bash
# Start all resources
./scripts/azure-deploy.sh --start
```

Storage accounts and Container Registry may still incur minimal charges when stopped.

## Updating Deployments

After making code changes, redeploy by running the script again:

```bash
./scripts/azure-deploy.sh
```

The script detects existing resources and updates them rather than recreating.

## Management Commands

```bash
# View Praxis logs (real-time)
az containerapp logs show -n praxis-app -g praxis-rg --follow

# View RabbitMQ logs
az container logs --name praxis-rabbitmq -g praxis-rg --follow

# Restart RabbitMQ
az container restart --name praxis-rabbitmq -g praxis-rg
```

## Troubleshooting

```bash
# Check Praxis app status
az containerapp show -n praxis-app -g praxis-rg --query properties.runningStatus

# View recent logs
az containerapp logs show -n praxis-app -g praxis-rg --tail 100
az container logs --name praxis-rabbitmq -g praxis-rg --tail 100

# Check RabbitMQ status
az container show --name praxis-rabbitmq -g praxis-rg --query instanceView.state

# Check PostgreSQL status
az postgres flexible-server show -n <server-name> -g praxis-rg --query state
```

## Security Best Practices

> **Warning**: Praxis has no built-in authentication or access control. Anyone who can reach the RabbitMQ endpoint can drive the deployment. You must protect access at the network or transport level.

### Protecting the RabbitMQ Endpoint

- **VNet Integration** — keep RabbitMQ on an internal network and reach it via VPN or Azure Bastion.
- **IP Allowlisting** — restrict the RabbitMQ Container Instance's public IP to known operator IPs.
- **TLS / strong credentials** — change the default `praxis:praxis` credentials and front RabbitMQ with TLS.

### Other Security Recommendations

1. **Change default passwords** - Set `PRAXIS_POSTGRES_PASS` for PostgreSQL. RabbitMQ credentials (`praxis`/`praxis`) are hardcoded in the deploy script with no environment-variable override, so changing them currently requires editing `scripts/azure-deploy.sh` directly
2. **Use Azure Key Vault** - Store secrets securely rather than in environment variables
3. **Enable diagnostic logging** - Send logs to Log Analytics for audit trails
4. **Regular updates** - Keep base images current

## Cleanup

Delete all resources:

```bash
./scripts/azure-deploy.sh --delete
```

This deletes:
- Container Instance (RabbitMQ)
- Container App (Praxis)
- PostgreSQL Flexible Server
- Azure Container Registry
- Storage Account
- Log Analytics Workspace
- Container App Environment
- Resource Group

Verify deletion:

```bash
az group list --query "[?name=='praxis-rg']" -o table
```
