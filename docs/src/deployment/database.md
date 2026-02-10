# Database Configuration

Praxis supports two database backends:

- **SQLite** (default) - Zero-configuration, single-instance deployments
- **PostgreSQL** - Production deployments, multiple service instances

## Quick Reference

| Feature | SQLite | PostgreSQL |
|---------|--------|------------|
| Setup | Automatic | Requires server |
| Multiple instances | No | Yes |
| Network storage (SMB/NFS) | No | Yes |
| Cloud deployments | No | Yes |
| Connection pooling | 1 connection | 10 connections |
| Best for | Local development | Production, cloud, teams |

## SQLite (Default)

No configuration required. The database file is created automatically at:

| Platform | Path |
|----------|------|
| Linux/macOS | `~/.praxis_operations.db` |
| Windows | `%USERPROFILE%\.praxis_operations.db` |

SQLite is configured with WAL journal mode and a 5-second busy timeout.

**Warning**: SQLite does not work reliably on network file systems (SMB, NFS, Azure Files, EFS). File locking mechanisms don't translate correctly over these protocols, leading to database corruption and "database is locked" errors. For cloud deployments with persistent storage, use PostgreSQL.

### Custom SQLite Path

```bash
export PRAXIS_DATABASE_URL=/path/to/custom.db
# or
export PRAXIS_DATABASE_URL=sqlite:///path/to/custom.db
```

## PostgreSQL

### Prerequisites

1. PostgreSQL 14+ server
2. A database created for Praxis
3. User with CREATE TABLE privileges

### Setup

Create the database:

```bash
createdb praxis
```

Configure the connection:

```bash
export PRAXIS_DATABASE_URL=postgresql://user:password@host:5432/praxis
```

The schema is created automatically on first run.

### Connection URL Format

```
postgresql://[user[:password]@][host][:port]/database[?options]
```

Examples:

```bash
# Local server, default port
postgresql://praxis:secret@localhost/praxis

# Remote server with port
postgresql://praxis:secret@db.example.com:5432/praxis

# With SSL mode
postgresql://praxis:secret@db.example.com:5432/praxis?sslmode=require
```

### SSL/TLS Configuration

For production deployments, enable SSL in the connection URL:

| Mode | Description |
|------|-------------|
| `sslmode=disable` | No SSL (not recommended) |
| `sslmode=prefer` | Try SSL, fall back to unencrypted |
| `sslmode=require` | Require SSL, don't verify certificate |
| `sslmode=verify-ca` | Require SSL, verify CA |
| `sslmode=verify-full` | Require SSL, verify CA and hostname |

Example with full verification:

```bash
export PRAXIS_DATABASE_URL="postgresql://user:pass@host:5432/praxis?sslmode=verify-full&sslrootcert=/path/to/ca.crt"
```

### Connection Pool Settings

PostgreSQL connections use these defaults:

| Setting | Value | Description |
|---------|-------|-------------|
| Max connections | 10 | Maximum pool size |
| Connect timeout | 30s | Time to establish connection |
| Idle timeout | 600s | Close idle connections after |

These are hardcoded but sufficient for most deployments. For high-traffic scenarios, tune PostgreSQL server settings (`max_connections`, `shared_buffers`) instead.

## Schema

The schema is created automatically. Key tables:

| Table | Purpose |
|-------|---------|
| `operations` | Semantic operation executions |
| `operation_definitions` | Stored operation templates |
| `intercepted_traffic` | Captured HTTP traffic |
| `intercept_rules` | Traffic matching rules |
| `traffic_matches` | Rule match results |
| `operation_chains` | Chain workflow definitions |
| `chain_executions` | Chain execution history |
| `recon_results` | Agent reconnaissance data |
| `event_log` | Centralized logging |
| `service_config` | Key-value configuration |
| `lua_agent_scripts` | Lua agent connector scripts |

Traffic data is automatically pruned after 7 days.

### Schema Migrations

Schema migrations run automatically on service startup. The service applies idempotent `ALTER TABLE` statements to add new columns introduced in newer versions. No manual migration steps are required when upgrading Praxis. The `service_config` table stores version tracking keys (e.g., `builtin_scripts_version`) to coordinate data migrations like updating built-in scripts.

## Migration: SQLite to PostgreSQL

Praxis doesn't include a built-in migration tool. To migrate:

1. Export data from SQLite:

```bash
sqlite3 ~/.praxis_operations.db .dump > praxis_dump.sql
```

2. Convert SQLite-specific syntax to PostgreSQL:
   - `INTEGER PRIMARY KEY` → `SERIAL PRIMARY KEY`
   - `BLOB` → `BYTEA`
   - Remove `AUTOINCREMENT`
   - Adjust date functions if used

3. Import to PostgreSQL:

```bash
psql -d praxis -f praxis_dump.sql
```

For most deployments, starting fresh with PostgreSQL is simpler than migrating.

## Multi-Instance and Cloud Deployments

PostgreSQL is required for:
- Multiple `praxis_service` instances (e.g., behind a load balancer)
- Cloud deployments (Azure Container Apps, AWS ECS, Kubernetes)
- Any deployment using network-attached storage

SQLite limitations:
- File locking doesn't work over SMB, NFS, Azure Files, or EFS
- Concurrent writes from multiple processes cause corruption
- "Database is locked" errors under load
- No recovery from partial writes on network storage

PostgreSQL handles:
- Concurrent connections from multiple instances
- Proper transaction isolation and row-level locking
- Network-transparent client/server architecture
- Connection pooling per instance

## Backup and Restore

### SQLite

```bash
# Backup
cp ~/.praxis_operations.db ~/.praxis_operations.db.backup

# Restore
cp ~/.praxis_operations.db.backup ~/.praxis_operations.db
```

### PostgreSQL

```bash
# Backup
pg_dump -Fc praxis > praxis_backup.dump

# Restore
pg_restore -d praxis praxis_backup.dump
```

For point-in-time recovery, configure PostgreSQL WAL archiving.

## Troubleshooting

### Connection Refused

```
Error: Connection refused (os error 111)
```

- Verify PostgreSQL is running: `pg_isready -h host -p 5432`
- Check firewall rules allow port 5432
- Verify `pg_hba.conf` allows connections from your IP

### Authentication Failed

```
Error: password authentication failed for user "praxis"
```

- Verify username and password in URL
- Check `pg_hba.conf` authentication method
- Ensure user exists: `\du` in psql

### Database Does Not Exist

```
Error: database "praxis" does not exist
```

Create it:

```bash
createdb praxis
# or
psql -c "CREATE DATABASE praxis;"
```

### SSL Required

```
Error: SSL connection is required
```

Add SSL mode to connection URL:

```bash
postgresql://user:pass@host:5432/praxis?sslmode=require
```

### SQLite Locked

```
Error: database is locked
```

- If using network storage (SMB, NFS, Azure Files): switch to PostgreSQL
- Only one `praxis_service` instance can use SQLite
- Close other connections (GUI tools, scripts)
- Check for zombie processes: `lsof ~/.praxis_operations.db`

## Performance Tuning

### PostgreSQL Server

For production workloads, tune these PostgreSQL settings:

```
# postgresql.conf
max_connections = 100
shared_buffers = 256MB
effective_cache_size = 768MB
maintenance_work_mem = 64MB
checkpoint_completion_target = 0.9
wal_buffers = 16MB
default_statistics_target = 100
random_page_cost = 1.1
effective_io_concurrency = 200
work_mem = 4MB
```

### Vacuum and Maintenance

PostgreSQL autovacuum handles routine maintenance. For large traffic volumes, consider:

```bash
# Manual vacuum after bulk deletes
psql -d praxis -c "VACUUM ANALYZE intercepted_traffic;"
```

### Indexing

The schema includes indexes for common queries. If you run custom queries against the database, add indexes as needed:

```sql
-- Example: index for custom report queries
CREATE INDEX idx_operations_agent ON operations(agent_short_name);
```
