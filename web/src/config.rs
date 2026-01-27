use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration manager with file persistence
pub struct ConfigManager {
    /// In-memory cache of config values
    cache: RwLock<HashMap<String, String>>,
    /// Path to the config file
    config_path: PathBuf,
}

impl ConfigManager {
    /// Create a new ConfigManager and load existing config from disk
    pub fn new() -> Arc<Self> {
        let config_path = Self::get_config_path();
        let manager = Arc::new(Self {
            cache: RwLock::new(HashMap::new()),
            config_path,
        });

        //
        // Load existing config synchronously on startup.
        //
        if let Ok(contents) = std::fs::read_to_string(&manager.config_path) {
            if let Ok(values) = serde_json::from_str::<HashMap<String, String>>(&contents) {
                common::log_info!("Loaded {} config values from {:?}", values.len(), manager.config_path);
                //
                // We can't use async here, so we'll use try_write.
                //
                if let Ok(mut cache) = manager.cache.try_write() {
                    *cache = values;
                }
            }
        } else {
            common::log_info!("No existing config file at {:?}", manager.config_path);
        }

        manager
    }

    /// Get the path to the config file (~/.praxis/config.json)
    fn get_config_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| {
            //
            // Fallback to current directory if home not found.
            //
            common::log_warn!("Could not determine home directory, using current directory");
            PathBuf::from(".")
        });

        let praxis_dir = home.join(".praxis");
        praxis_dir.join("config.json")
    }

    /// Ensure the config directory exists
    fn ensure_config_dir(&self) -> std::io::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// Get config values for the given keys
    pub async fn get(&self, keys: &[&str]) -> HashMap<String, String> {
        let cache = self.cache.read().await;
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = cache.get(*key) {
                result.insert(key.to_string(), value.clone());
            }
        }
        result
    }

    /// Get a single config value
    #[allow(dead_code)]
    pub async fn get_one(&self, key: &str) -> Option<String> {
        let cache = self.cache.read().await;
        cache.get(key).cloned()
    }

    /// Set config values and persist to disk
    pub async fn set(&self, values: HashMap<String, String>) -> Result<(), String> {
        //
        // Update cache.
        //
        {
            let mut cache = self.cache.write().await;
            for (k, v) in values {
                if v.is_empty() {
                    cache.remove(&k);
                } else {
                    cache.insert(k, v);
                }
            }
        }

        //
        // Persist to disk.
        //
        self.save().await
    }

    /// Save the current config to disk
    async fn save(&self) -> Result<(), String> {
        //
        // Ensure directory exists.
        //
        self.ensure_config_dir()
            .map_err(|e| format!("Failed to create config directory: {}", e))?;

        //
        // Serialize and write.
        //
        let cache = self.cache.read().await;
        let json = serde_json::to_string_pretty(&*cache)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        tokio::fs::write(&self.config_path, json)
            .await
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        common::log_info!("Saved {} config values to {:?}", cache.len(), self.config_path);
        Ok(())
    }

    /// Get all config values (for debugging/admin)
    #[allow(dead_code)]
    pub async fn get_all(&self) -> HashMap<String, String> {
        let cache = self.cache.read().await;
        cache.clone()
    }

    /// Load config from disk (useful for reloading)
    #[allow(dead_code)]
    pub async fn reload(&self) -> Result<(), String> {
        let contents = tokio::fs::read_to_string(&self.config_path)
            .await
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let values: HashMap<String, String> = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        let mut cache = self.cache.write().await;
        *cache = values;

        common::log_info!("Reloaded {} config values from {:?}", cache.len(), self.config_path);
        Ok(())
    }
}
