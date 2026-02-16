use std::collections::HashMap;

use super::AppState;

impl AppState {
    pub async fn update_config(&self, values: HashMap<String, String>) {
        let mut cache = self.config_cache.write().await;
        for (k, v) in values {
            cache.insert(k, v);
        }
        drop(cache);
        self.config_notify.notify_waiters();
    }

    #[allow(dead_code)]
    pub async fn get_config(&self, keys: &[&str]) -> HashMap<String, String> {
        let cache = self.config_cache.read().await;
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = cache.get(*key) {
                result.insert(key.to_string(), value.clone());
            }
        }
        result
    }
}
