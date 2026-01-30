//
// Database operations for discovered LLM endpoints.
//

use anyhow::Result;
use chrono::Utc;
use common::DiscoveredLlmEndpoint;
use sqlx::Row;

use super::{Database, DatabasePool};

impl Database {
    /// Insert or update a discovered LLM endpoint
    pub async fn upsert_discovered_endpoint(&self, endpoint: &DiscoveredLlmEndpoint) -> Result<()> {
        let models_json =
            serde_json::to_string(&endpoint.models).unwrap_or_else(|_| "[]".to_string());

        let sql = "INSERT INTO discovered_endpoints (
                id, node_id, ip_address, domain, port, is_https,
                models, base_url, api_key, discovered_at, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT(id) DO UPDATE SET
                node_id = $2,
                ip_address = $3,
                domain = $4,
                port = $5,
                is_https = $6,
                models = $7,
                base_url = $8,
                api_key = $9,
                discovered_at = $10";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                sqlx::query(sql)
                    .bind(&endpoint.id)
                    .bind(&endpoint.node_id)
                    .bind(&endpoint.ip_address)
                    .bind(&endpoint.domain)
                    .bind(endpoint.port as i32)
                    .bind(endpoint.is_https)
                    .bind(&models_json)
                    .bind(&endpoint.base_url)
                    .bind(&endpoint.api_key)
                    .bind(endpoint.discovered_at.to_rfc3339())
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await?;
            }
            DatabasePool::Postgres(pool) => {
                sqlx::query(sql)
                    .bind(&endpoint.id)
                    .bind(&endpoint.node_id)
                    .bind(&endpoint.ip_address)
                    .bind(&endpoint.domain)
                    .bind(endpoint.port as i32)
                    .bind(if endpoint.is_https { 1i16 } else { 0i16 })
                    .bind(&models_json)
                    .bind(&endpoint.base_url)
                    .bind(&endpoint.api_key)
                    .bind(endpoint.discovered_at.to_rfc3339())
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await?;
            }
        }

        Ok(())
    }

    /// Get discovered endpoints for a specific node
    pub async fn get_discovered_endpoints(
        &self,
        node_id: &str,
    ) -> Result<Vec<DiscoveredLlmEndpoint>> {
        let sql = "SELECT id, node_id, ip_address, domain, port, is_https,
                models, base_url, api_key, discovered_at
             FROM discovered_endpoints
             WHERE node_id = $1
             ORDER BY discovered_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).bind(node_id).fetch_all(pool).await?;
                let mut endpoints = Vec::new();
                for row in rows {
                    endpoints.push(parse_endpoint_row_sqlite(&row)?);
                }
                Ok(endpoints)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).bind(node_id).fetch_all(pool).await?;
                let mut endpoints = Vec::new();
                for row in rows {
                    endpoints.push(parse_endpoint_row_postgres(&row)?);
                }
                Ok(endpoints)
            }
        }
    }

    /// Get all discovered endpoints across all nodes
    pub async fn get_all_discovered_endpoints(&self) -> Result<Vec<DiscoveredLlmEndpoint>> {
        let sql = "SELECT id, node_id, ip_address, domain, port, is_https,
                models, base_url, api_key, discovered_at
             FROM discovered_endpoints
             ORDER BY discovered_at DESC";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut endpoints = Vec::new();
                for row in rows {
                    endpoints.push(parse_endpoint_row_sqlite(&row)?);
                }
                Ok(endpoints)
            }
            DatabasePool::Postgres(pool) => {
                let rows = sqlx::query(sql).fetch_all(pool).await?;
                let mut endpoints = Vec::new();
                for row in rows {
                    endpoints.push(parse_endpoint_row_postgres(&row)?);
                }
                Ok(endpoints)
            }
        }
    }

    /// Delete a discovered endpoint by ID
    #[allow(dead_code)]
    pub async fn delete_discovered_endpoint(&self, id: &str) -> Result<bool> {
        let sql = "DELETE FROM discovered_endpoints WHERE id = $1";

        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(id)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(rows_affected > 0)
    }

    /// Clear all discovered endpoints for a node
    #[allow(dead_code)]
    pub async fn clear_discovered_endpoints(&self, node_id: &str) -> Result<usize> {
        let sql = "DELETE FROM discovered_endpoints WHERE node_id = $1";

        let rows_affected = match &self.pool {
            DatabasePool::Sqlite(pool) => sqlx::query(sql)
                .bind(node_id)
                .execute(pool)
                .await?
                .rows_affected(),
            DatabasePool::Postgres(pool) => sqlx::query(sql)
                .bind(node_id)
                .execute(pool)
                .await?
                .rows_affected(),
        };

        Ok(rows_affected as usize)
    }

    /// Get a specific discovered endpoint by ID
    #[allow(dead_code)]
    pub async fn get_discovered_endpoint(&self, id: &str) -> Result<Option<DiscoveredLlmEndpoint>> {
        let sql = "SELECT id, node_id, ip_address, domain, port, is_https,
                models, base_url, api_key, discovered_at
             FROM discovered_endpoints
             WHERE id = $1";

        match &self.pool {
            DatabasePool::Sqlite(pool) => {
                let row = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                match row {
                    Some(row) => Ok(Some(parse_endpoint_row_sqlite(&row)?)),
                    None => Ok(None),
                }
            }
            DatabasePool::Postgres(pool) => {
                let row = sqlx::query(sql).bind(id).fetch_optional(pool).await?;
                match row {
                    Some(row) => Ok(Some(parse_endpoint_row_postgres(&row)?)),
                    None => Ok(None),
                }
            }
        }
    }
}

//
// Helper functions.
//

fn parse_endpoint_row_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<DiscoveredLlmEndpoint> {
    let id: String = row.get(0);
    let node_id: String = row.get(1);
    let ip_address: String = row.get(2);
    let domain: Option<String> = row.get(3);
    let port: i32 = row.get(4);
    let is_https: bool = row.get(5);
    let models_json: String = row.get(6);
    let base_url: String = row.get(7);
    let api_key: Option<String> = row.get(8);
    let discovered_at_str: String = row.get(9);

    let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
    let discovered_at = chrono::DateTime::parse_from_rfc3339(&discovered_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(DiscoveredLlmEndpoint {
        id,
        node_id,
        ip_address,
        domain,
        port: port as u16,
        is_https,
        models,
        base_url,
        api_key,
        discovered_at,
    })
}

fn parse_endpoint_row_postgres(row: &sqlx::postgres::PgRow) -> Result<DiscoveredLlmEndpoint> {
    let id: String = row.get(0);
    let node_id: String = row.get(1);
    let ip_address: String = row.get(2);
    let domain: Option<String> = row.get(3);
    let port: i32 = row.get(4);
    let is_https: i16 = row.get(5);
    let models_json: String = row.get(6);
    let base_url: String = row.get(7);
    let api_key: Option<String> = row.get(8);
    let discovered_at_str: String = row.get(9);

    let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
    let discovered_at = chrono::DateTime::parse_from_rfc3339(&discovered_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(DiscoveredLlmEndpoint {
        id,
        node_id,
        ip_address,
        domain,
        port: port as u16,
        is_https: is_https != 0,
        models,
        base_url,
        api_key,
        discovered_at,
    })
}
