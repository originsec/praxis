//
// Database operations for discovered LLM endpoints.
//

use chrono::Utc;
use common::DiscoveredLlmEndpoint;
use rusqlite::{params, Result as SqliteResult};

use super::Database;

impl Database {
    /// Insert or update a discovered LLM endpoint
    pub fn upsert_discovered_endpoint(&self, endpoint: &DiscoveredLlmEndpoint) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

        let models_json = serde_json::to_string(&endpoint.models).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT OR REPLACE INTO discovered_endpoints (
                id, node_id, ip_address, domain, port, is_https,
                models, base_url, api_key, discovered_at, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                endpoint.id,
                endpoint.node_id,
                endpoint.ip_address,
                endpoint.domain,
                endpoint.port,
                endpoint.is_https,
                models_json,
                endpoint.base_url,
                endpoint.api_key,
                endpoint.discovered_at.to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Get discovered endpoints for a specific node
    pub fn get_discovered_endpoints(&self, node_id: &str) -> SqliteResult<Vec<DiscoveredLlmEndpoint>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, node_id, ip_address, domain, port, is_https,
                    models, base_url, api_key, discovered_at
             FROM discovered_endpoints
             WHERE node_id = ?1
             ORDER BY discovered_at DESC",
        )?;

        let rows = stmt.query_map([node_id], |row| {
            let models_json: String = row.get(6)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();

            let discovered_at_str: String = row.get(9)?;
            let discovered_at = chrono::DateTime::parse_from_rfc3339(&discovered_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(DiscoveredLlmEndpoint {
                id: row.get(0)?,
                node_id: row.get(1)?,
                ip_address: row.get(2)?,
                domain: row.get(3)?,
                port: row.get(4)?,
                is_https: row.get(5)?,
                models,
                base_url: row.get(7)?,
                api_key: row.get(8)?,
                discovered_at,
            })
        })?;

        rows.collect()
    }

    /// Get all discovered endpoints across all nodes
    pub fn get_all_discovered_endpoints(&self) -> SqliteResult<Vec<DiscoveredLlmEndpoint>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, node_id, ip_address, domain, port, is_https,
                    models, base_url, api_key, discovered_at
             FROM discovered_endpoints
             ORDER BY discovered_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let models_json: String = row.get(6)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();

            let discovered_at_str: String = row.get(9)?;
            let discovered_at = chrono::DateTime::parse_from_rfc3339(&discovered_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(DiscoveredLlmEndpoint {
                id: row.get(0)?,
                node_id: row.get(1)?,
                ip_address: row.get(2)?,
                domain: row.get(3)?,
                port: row.get(4)?,
                is_https: row.get(5)?,
                models,
                base_url: row.get(7)?,
                api_key: row.get(8)?,
                discovered_at,
            })
        })?;

        rows.collect()
    }

    /// Delete a discovered endpoint by ID
    pub fn delete_discovered_endpoint(&self, id: &str) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM discovered_endpoints WHERE id = ?1",
            [id],
        )?;
        Ok(rows_affected > 0)
    }

    /// Clear all discovered endpoints for a node
    pub fn clear_discovered_endpoints(&self, node_id: &str) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let rows_affected = conn.execute(
            "DELETE FROM discovered_endpoints WHERE node_id = ?1",
            [node_id],
        )?;
        Ok(rows_affected)
    }

    /// Get a specific discovered endpoint by ID
    pub fn get_discovered_endpoint(&self, id: &str) -> SqliteResult<Option<DiscoveredLlmEndpoint>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, node_id, ip_address, domain, port, is_https,
                    models, base_url, api_key, discovered_at
             FROM discovered_endpoints
             WHERE id = ?1",
        )?;

        let mut rows = stmt.query([id])?;

        if let Some(row) = rows.next()? {
            let models_json: String = row.get(6)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();

            let discovered_at_str: String = row.get(9)?;
            let discovered_at = chrono::DateTime::parse_from_rfc3339(&discovered_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Some(DiscoveredLlmEndpoint {
                id: row.get(0)?,
                node_id: row.get(1)?,
                ip_address: row.get(2)?,
                domain: row.get(3)?,
                port: row.get(4)?,
                is_https: row.get(5)?,
                models,
                base_url: row.get(7)?,
                api_key: row.get(8)?,
                discovered_at,
            }))
        } else {
            Ok(None)
        }
    }
}
