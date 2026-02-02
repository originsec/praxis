# Azure Deployment

This guide covers deploying Praxis to Azure.

## Architecture

A typical Azure deployment:

```
┌────────────────────────────────────────────────────┐
│                       Azure                        │
│                                                    │
│   ┌──────────────┐    ┌──────────────────────┐    │
│   │  App Service │    │  Azure Service Bus   │    │
│   │  (Web + Svc) │◄───│  (or self-hosted RMQ)│    │
│   └──────┬───────┘    └──────────────────────┘    │
│          │                                         │
│   ┌──────▼───────┐                                │
│   │ Azure SQL or │                                │
│   │  PostgreSQL  │                                │
│   └──────────────┘                                │
│                                                    │
└────────────────────────────────────────────────────┘
             │
             │ Internet
             │
       ┌─────▼─────┐
       │   Nodes   │
       │ (Targets) │
       └───────────┘
```

## Components

### RabbitMQ

Options:
1. **CloudAMQP** - Managed RabbitMQ service
2. **Azure Container Instance** - Self-hosted RabbitMQ
3. **Azure VM** - Traditional VM deployment

CloudAMQP is simplest for getting started.

### Database

Options:
1. **Azure Database for PostgreSQL** - Managed PostgreSQL
2. **Azure SQL** - Managed SQL (requires minor schema changes)
3. **SQLite** - File-based, simplest but limited

For production, use managed PostgreSQL.

### Web + Service

Deploy as a single container to:
- Azure App Service
- Azure Container Instances
- Azure Kubernetes Service

## Deployment Steps

### 1. Set Up RabbitMQ

Using CloudAMQP:
1. Create account at cloudamqp.com
2. Create a new instance (Little Lemur tier for testing)
3. Note the AMQP URL

Or deploy to Azure Container Instance:
```bash
az container create \
  --resource-group praxis-rg \
  --name praxis-rabbitmq \
  --image rabbitmq:3-management \
  --ports 5672 15672 \
  --environment-variables \
    RABBITMQ_DEFAULT_USER=praxis \
    RABBITMQ_DEFAULT_PASS=<strong-password>
```

### 2. Set Up Database

Create Azure Database for PostgreSQL:
```bash
az postgres flexible-server create \
  --resource-group praxis-rg \
  --name praxis-db \
  --admin-user praxis \
  --admin-password <strong-password> \
  --sku-name Standard_B1ms

az postgres flexible-server db create \
  --resource-group praxis-rg \
  --server-name praxis-db \
  --database-name praxis
```

### 3. Build Container

Create a Dockerfile (already in repo):
```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/praxis_service /usr/local/bin/
COPY --from=builder /app/target/release/praxis_web /usr/local/bin/
ENTRYPOINT ["/usr/local/bin/praxis_web"]
```

Build and push:
```bash
az acr build \
  --registry praxisacr \
  --image praxis:latest .
```

### 4. Deploy to App Service

Create App Service:
```bash
az webapp create \
  --resource-group praxis-rg \
  --plan praxis-plan \
  --name praxis-app \
  --deployment-container-image-name praxisacr.azurecr.io/praxis:latest
```

Configure environment:
```bash
az webapp config appsettings set \
  --resource-group praxis-rg \
  --name praxis-app \
  --settings \
    PRAXIS_RABBITMQ_URL="amqps://user:pass@host.cloudamqp.com/vhost" \
    PRAXIS_DATABASE_URL="postgresql://praxis:pass@praxis-db.postgres.database.azure.com/praxis" \
    RUST_LOG="info"
```

### 5. Configure Networking

Ensure nodes can reach:
- RabbitMQ (port 5672 or 5671 for TLS)
- Web UI (port 443 through App Service)

For nodes on-premises or in other clouds, consider:
- Azure VPN Gateway
- ExpressRoute
- Public endpoint with firewall rules

## Node Deployment

Nodes deploy to target machines (Windows, Linux):

1. Download node binary from Settings in the web UI
2. Configure environment:
   ```bash
   export PRAXIS_RABBITMQ_URL="amqps://..."
   ```
3. Run the node binary

For persistent deployment, create a systemd service (Linux) or Windows Service.

## Security Considerations

### RabbitMQ

- Use TLS (amqps://)
- Strong passwords
- Network security groups

### Database

- Enable SSL
- Private endpoint (no public access)
- Network security groups

### Web UI

- App Service authentication
- Azure AD integration
- IP restrictions

### Secrets

- Use Azure Key Vault
- Reference secrets in App Service config
- Don't commit credentials

## Scaling

### Horizontal Scaling

- App Service supports scaling out
- Multiple service instances work with PostgreSQL
- RabbitMQ handles message distribution

### Database

- Scale up the PostgreSQL tier
- Enable connection pooling
- Consider read replicas for heavy read loads

## Monitoring

### Application Insights

Add to App Service for:
- Request tracing
- Error tracking
- Performance metrics

### Log Analytics

Configure diagnostic settings to send:
- Container logs
- App Service logs
- PostgreSQL logs

### Alerts

Set up alerts for:
- High error rate
- Connection failures
- Resource exhaustion

## Cost Optimization

### Development/Testing

- Use consumption-based plans
- Scale down when not in use
- Use Basic tier PostgreSQL

### Production

- Reserved instances for predictable workloads
- Right-size based on actual usage
- Monitor and adjust regularly
