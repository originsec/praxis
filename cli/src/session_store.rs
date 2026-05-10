use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

//
// On-disk record for a TUI orchestrator session. The service holds no
// orchestrator state; the TUI persists each completed turn here so that
// `praxis --resume` and `praxis --continue` can list and reseed prior
// conversations.
//

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub role: String,    // "user" | "assistant"
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub session_id: String,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub messages: Vec<StoredMessage>,
}

impl StoredSession {
    pub fn new(session_id: String) -> Self {
        let now = now_ms();
        Self {
            session_id,
            created_at_ms: now,
            updated_at_ms: now,
            provider: None,
            model: None,
            messages: Vec::new(),
        }
    }

    pub fn first_user_text(&self) -> Option<&str> {
        self.messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| m.text.as_str())
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn sessions_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(home.join(".praxis").join("sessions"))
}

pub fn save(session: &StoredSession) -> Result<PathBuf> {
    let dir = sessions_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", session.session_id));
    let mut session = session.clone();
    session.updated_at_ms = now_ms();
    fs::write(&path, serde_json::to_string_pretty(&session)?)?;
    Ok(path)
}

pub fn load_path(path: &std::path::Path) -> Result<StoredSession> {
    let s = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

//
// List sessions sorted newest-first by updated_at_ms.
//

pub fn list() -> Result<Vec<StoredSession>> {
    let dir = match sessions_dir() {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out: Vec<StoredSession> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(s) = load_path(&path) {
            out.push(s);
        }
    }
    out.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    Ok(out)
}

pub fn most_recent() -> Result<Option<StoredSession>> {
    Ok(list()?.into_iter().next())
}
