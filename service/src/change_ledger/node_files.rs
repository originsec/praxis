use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use common::AgentFileType;
use common::acp_ext::{EXT_PRAXIS_SNAPSHOT_FILES, EXT_PRAXIS_WRITE_FILE};
use lapin::Channel;
use serde::Deserialize;
use serde_json::json;

use crate::acp_node_proxy::AcpNodeProxy;

//
// File I/O the ledger needs from a node: a full-content snapshot (to record
// before-content and check drift) and a compare-and-swap write (so a file
// changed between snapshot and write is never clobbered). Abstracted behind
// a trait so the ledger logic can be exercised against a local directory in
// tests.
//

const NODE_FILE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct FileSnapshot {
    pub path: String,
    pub exists: bool,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WriteResult {
    Written,
    //
    // The file no longer matched the expected hash; nothing was written.
    //
    Conflict,
}

#[async_trait]
pub trait NodeFiles: Send + Sync {
    async fn snapshot(&self, node_id: &str, paths: &[String]) -> Result<Vec<FileSnapshot>>;

    //
    // Write `content` (None deletes the file) only if the file currently
    // hashes to `expected_sha256` (None = must not exist).
    //

    async fn write(
        &self,
        node_id: &str,
        path: &str,
        expected_sha256: Option<&str>,
        content: Option<&str>,
    ) -> Result<WriteResult>;
}

pub struct ProxyNodeFiles {
    proxy: Arc<AcpNodeProxy>,
    channel: Channel,
}

impl ProxyNodeFiles {
    pub fn new(proxy: Arc<AcpNodeProxy>, channel: Channel) -> Self {
        Self { proxy, channel }
    }
}

#[derive(Deserialize)]
struct SnapshotResponse {
    #[serde(default)]
    files: Vec<FileSnapshot>,
    #[serde(default)]
    error: Option<String>,
}

#[async_trait]
impl NodeFiles for ProxyNodeFiles {
    async fn snapshot(&self, node_id: &str, paths: &[String]) -> Result<Vec<FileSnapshot>> {
        let result = self
            .proxy
            .request_with_timeout(
                &self.channel,
                node_id,
                EXT_PRAXIS_SNAPSHOT_FILES,
                json!({ "paths": paths }),
                NODE_FILE_TIMEOUT,
            )
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("-32601") || msg.to_lowercase().contains("method not found") {
                    anyhow!(
                        "node {} does not support the change ledger (upgrade the node)",
                        common::short_id(node_id)
                    )
                } else {
                    e
                }
            })?;
        let parsed: SnapshotResponse = serde_json::from_value(result)?;
        if let Some(err) = parsed.error {
            return Err(anyhow!(err));
        }
        Ok(parsed.files)
    }

    async fn write(
        &self,
        node_id: &str,
        path: &str,
        expected_sha256: Option<&str>,
        content: Option<&str>,
    ) -> Result<WriteResult> {
        let result = self
            .proxy
            .request_with_timeout(
                &self.channel,
                node_id,
                EXT_PRAXIS_WRITE_FILE,
                json!({
                    "file_type": AgentFileType::Config,
                    "path": path,
                    "contents": content.unwrap_or(""),
                    "delete": content.is_none(),
                    "expected_sha256": expected_sha256,
                    "expect_absent": expected_sha256.is_none(),
                }),
                NODE_FILE_TIMEOUT,
            )
            .await?;
        if result.get("conflict").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(WriteResult::Conflict);
        }
        if result.get("success").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(WriteResult::Written);
        }
        Err(anyhow!(
            "{}",
            result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("write failed")
        ))
    }
}

//
// Test double: maps node paths onto a local directory, using the same CAS
// semantics the node implements.
//

#[cfg(test)]
pub mod local {
    use super::*;
    use common::config_change::sha256_hex;
    use std::sync::atomic::{AtomicBool, Ordering};

    pub struct LocalFiles {
        pub offline: AtomicBool,
    }

    impl LocalFiles {
        pub fn new() -> Self {
            Self {
                offline: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl NodeFiles for LocalFiles {
        async fn snapshot(&self, _node_id: &str, paths: &[String]) -> Result<Vec<FileSnapshot>> {
            if self.offline.load(Ordering::Relaxed) {
                return Err(anyhow!("node offline"));
            }
            Ok(paths
                .iter()
                .map(|p| match std::fs::read_to_string(p) {
                    Ok(c) => FileSnapshot {
                        path: p.clone(),
                        exists: true,
                        sha256: Some(sha256_hex(c.as_bytes())),
                        content: Some(c),
                        error: None,
                    },
                    Err(_) => FileSnapshot {
                        path: p.clone(),
                        exists: false,
                        sha256: None,
                        content: None,
                        error: None,
                    },
                })
                .collect())
        }

        async fn write(
            &self,
            _node_id: &str,
            path: &str,
            expected_sha256: Option<&str>,
            content: Option<&str>,
        ) -> Result<WriteResult> {
            let current = std::fs::read(path).ok().map(|b| sha256_hex(&b));
            if current.as_deref() != expected_sha256 {
                return Ok(WriteResult::Conflict);
            }
            match content {
                Some(c) => std::fs::write(path, c)?,
                None => std::fs::remove_file(path)?,
            }
            Ok(WriteResult::Written)
        }
    }
}
