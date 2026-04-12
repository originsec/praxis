use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CliState {
    pub client_id: Option<String>,
}

impl CliState {
    fn state_file() -> Result<PathBuf> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        Ok(home.join(".praxis").join("cli.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::state_file()?;
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let state: CliState = serde_json::from_str(&content)?;
            Ok(state)
        } else {
            Ok(CliState::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::state_file()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn clear() -> Result<()> {
        let path = Self::state_file()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn get_or_create_client_id(&mut self) -> Result<String> {
        if let Some(ref id) = self.client_id {
            if !id.starts_with("cli_") {
                let prefixed = format!("cli_{}", &id[..8.min(id.len())]);
                self.client_id = Some(prefixed.clone());
                self.save()?;
                return Ok(prefixed);
            }
            Ok(id.clone())
        } else {
            let uid = uuid::Uuid::new_v4().to_string();
            let id = format!("cli_{}", &uid[..8]);
            self.client_id = Some(id.clone());
            self.save()?;
            Ok(id)
        }
    }
}
