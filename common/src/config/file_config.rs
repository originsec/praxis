use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// A generic configuration backed by a key=value file
///
/// # Format
///
/// The configuration file uses a simple key=value format:
/// ```text
/// api_key=your_key_here
/// provider=anthropic
/// model=claude-haiku-4-5
/// # Comments start with #
/// ```
#[derive(Debug, Clone)]
pub struct FileConfig {
    /// Path to the configuration file
    file_path: PathBuf,
    /// In-memory configuration data
    data: HashMap<String, String>,
}

impl FileConfig {
    /// Create a new FileConfig with the specified file path
    ///
    /// If the file doesn't exist, it will be created on first save.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the configuration file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use common::config::FileConfig;
    /// use std::path::PathBuf;
    ///
    /// let config = FileConfig::new(PathBuf::from("~/.myapp_config"));
    /// ```
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            data: HashMap::new(),
        }
    }

    /// Load configuration from file
    ///
    /// If the file doesn't exist, returns an empty configuration.
    /// Lines starting with # are treated as comments and ignored.
    /// Empty lines are ignored.
    /// Escaped newlines (\\n) in values are converted back to actual newlines.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read.
    pub fn load(&mut self) -> io::Result<()> {
        if !self.file_path.exists() {
            self.data = HashMap::new();
            return Ok(());
        }

        let content = fs::read_to_string(&self.file_path)?;
        let mut config = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                //
                // Unescape newlines and backslashes.
                //
                let unescaped_value = value.trim().replace("\\n", "\n").replace("\\\\", "\\");
                config.insert(key.trim().to_string(), unescaped_value);
            }
        }

        self.data = config;
        Ok(())
    }

    /// Save configuration to file
    ///
    /// Overwrites the existing file. Keys are sorted alphabetically.
    /// Newlines and backslashes in values are escaped to allow multi-line values.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn save(&self) -> io::Result<()> {
        let mut lines: Vec<String> = self
            .data
            .iter()
            .map(|(key, value)| {
                //
                // Escape backslashes first, then newlines.
                //
                let escaped_value = value.replace('\\', "\\\\").replace('\n', "\\n");
                format!("{}={}", key, escaped_value)
            })
            .collect();

        //
        // Sort for consistency.
        //
        lines.sort();

        fs::write(&self.file_path, lines.join("\n") + "\n")
    }

    /// Get a configuration value by key
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key to look up
    ///
    /// # Returns
    ///
    /// Some(value) if the key exists, None otherwise
    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    /// Set a configuration value
    ///
    /// Note: This only updates the in-memory configuration.
    /// Call `save()` to persist changes to disk.
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key
    /// * `value` - The value to set
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data.insert(key.into(), value.into());
    }

    /// Remove a configuration key
    ///
    /// Note: This only updates the in-memory configuration.
    /// Call `save()` to persist changes to disk.
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key to remove
    ///
    /// # Returns
    ///
    /// The previous value if it existed, None otherwise
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.data.remove(key)
    }

    /// Check if a key exists in the configuration
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key to check
    ///
    /// # Returns
    ///
    /// true if the key exists, false otherwise
    pub fn contains_key(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Get all keys in the configuration
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.data.keys()
    }

    /// Get all key-value pairs in the configuration
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.data.iter()
    }

    /// Clear all configuration values
    ///
    /// Note: This only updates the in-memory configuration.
    /// Call `save()` to persist changes to disk.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Get the path to the configuration file
    pub fn path(&self) -> &PathBuf {
        &self.file_path
    }
}

/// Helper function to find a config file by trying multiple paths
///
/// Searches for a file in the given paths in order and returns the first one that exists.
/// If none exist, returns the first path (for creating a new config file).
///
/// # Arguments
///
/// * `paths` - Vector of paths to try
///
/// # Returns
///
/// The first existing path, or the first path if none exist
///
/// # Examples
///
/// ```no_run
/// use common::config::find_config_file;
/// use std::path::PathBuf;
///
/// let paths = vec![
///     PathBuf::from(".myapp_config"),  // Current directory
///     PathBuf::from("/home/user/.myapp_config"),  // Home directory
/// ];
///
/// let config_path = find_config_file(paths);
/// ```
pub fn find_config_file(paths: Vec<PathBuf>) -> PathBuf {
    for path in &paths {
        if path.exists() {
            return path.clone();
        }
    }
    paths.into_iter().next().unwrap_or_default()
}

/// Load a configuration file from multiple possible locations
///
/// Tries each path in order and loads from the first one that exists.
/// If none exist, returns an empty configuration with the first path.
///
/// # Arguments
///
/// * `paths` - Vector of paths to try
///
/// # Returns
///
/// A FileConfig loaded from the first existing path, or empty if none exist
pub fn load_from_paths(paths: Vec<PathBuf>) -> io::Result<FileConfig> {
    let config_path = find_config_file(paths);
    let mut config = FileConfig::new(config_path);
    config.load()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_config_basic_operations() {
        let temp_path = PathBuf::from("/tmp/test_praxis_common_config");
        let mut config = FileConfig::new(temp_path.clone());

        //
        // Set some values.
        //
        config.set("key1", "value1");
        config.set("key2", "value2");

        assert_eq!(config.get("key1"), Some(&"value1".to_string()));
        assert_eq!(config.get("key2"), Some(&"value2".to_string()));
        assert_eq!(config.get("key3"), None);

        //
        // Test contains_key.
        //
        assert!(config.contains_key("key1"));
        assert!(!config.contains_key("key3"));

        //
        // Test remove.
        //
        assert_eq!(config.remove("key1"), Some("value1".to_string()));
        assert!(!config.contains_key("key1"));

        //
        // Cleanup.
        //
        let _ = fs::remove_file(&temp_path);
    }

    #[test]
    fn test_config_save_and_load() {
        let temp_path = PathBuf::from("/tmp/test_praxis_common_config2");

        //
        // Create and save config.
        //
        {
            let mut config = FileConfig::new(temp_path.clone());
            config.set("api_key", "test_key");
            config.set("provider", "anthropic");
            config.save().unwrap();
        }

        //
        // Load and verify.
        //
        {
            let mut config = FileConfig::new(temp_path.clone());
            config.load().unwrap();
            assert_eq!(config.get("api_key"), Some(&"test_key".to_string()));
            assert_eq!(config.get("provider"), Some(&"anthropic".to_string()));
        }

        //
        // Cleanup.
        //
        let _ = fs::remove_file(&temp_path);
    }

    #[test]
    fn test_config_comments_and_empty_lines() {
        let temp_path = PathBuf::from("/tmp/test_praxis_common_config3");

        //
        // Write config with comments.
        //
        fs::write(
            &temp_path,
            "# This is a comment\nkey1=value1\n\n# Another comment\nkey2=value2\n",
        )
        .unwrap();

        //
        // Load and verify.
        //
        let mut config = FileConfig::new(temp_path.clone());
        config.load().unwrap();
        assert_eq!(config.get("key1"), Some(&"value1".to_string()));
        assert_eq!(config.get("key2"), Some(&"value2".to_string()));

        //
        // Cleanup.
        //
        let _ = fs::remove_file(&temp_path);
    }

    #[test]
    fn test_config_multiline_values() {
        let temp_path = PathBuf::from("/tmp/test_praxis_common_config4");

        //
        // Create config with multi-line value.
        //
        {
            let mut config = FileConfig::new(temp_path.clone());
            config.set("prompt", "Line 1\nLine 2\nLine 3");
            config.set("normal", "single line");
            config.set("with_backslash", "path\\to\\file");
            config.save().unwrap();
        }

        //
        // Load and verify.
        //
        {
            let mut config = FileConfig::new(temp_path.clone());
            config.load().unwrap();
            assert_eq!(
                config.get("prompt"),
                Some(&"Line 1\nLine 2\nLine 3".to_string())
            );
            assert_eq!(config.get("normal"), Some(&"single line".to_string()));
            assert_eq!(
                config.get("with_backslash"),
                Some(&"path\\to\\file".to_string())
            );
        }

        //
        // Cleanup.
        //
        let _ = fs::remove_file(&temp_path);
    }
}
