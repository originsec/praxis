//!
//! Database configuration module.
//!
//! Supports SQLite (default) and PostgreSQL backends via environment variable:
//! - PRAXIS_DATABASE_URL: Full connection URL (postgres://... or sqlite://... or file path)
//! - Default: ~/.praxis/operations.db (SQLite)
//!

use std::path::PathBuf;

/// Database backend configuration
#[derive(Debug, Clone)]
pub enum DatabaseConfig {
    /// SQLite database with file path
    Sqlite { path: PathBuf },
    /// PostgreSQL database with connection URL
    Postgres { url: String },
}

impl DatabaseConfig {
    /// Parse database configuration from environment variables.
    ///
    /// PRAXIS_DATABASE_URL formats:
    /// - postgres://user:pass@host:5432/dbname - PostgreSQL connection
    /// - postgresql://user:pass@host:5432/dbname - PostgreSQL connection
    /// - sqlite:///path/to/file.db - SQLite file path
    /// - /path/to/file.db - SQLite file path (implicit)
    ///
    /// Default: ~/.praxis/operations.db (SQLite)
    pub fn from_env() -> Self {
        if let Ok(url) = std::env::var("PRAXIS_DATABASE_URL") {
            if url.starts_with("postgres://") || url.starts_with("postgresql://") {
                return DatabaseConfig::Postgres { url };
            } else if url.starts_with("sqlite://") {
                //
                // Extract path from sqlite:// URL.
                //
                let path = url.strip_prefix("sqlite://").unwrap_or(&url);
                return DatabaseConfig::Sqlite {
                    path: PathBuf::from(path),
                };
            } else {
                //
                // Assume it's a file path for SQLite.
                //
                return DatabaseConfig::Sqlite {
                    path: PathBuf::from(url),
                };
            }
        }

        //
        // Default to SQLite under ~/.praxis/. The parent directory is
        // created by the migration runner before the connection is
        // opened.
        //
        let path = dirs::home_dir()
            .expect("Failed to get home directory")
            .join(".praxis")
            .join("operations.db");

        DatabaseConfig::Sqlite { path }
    }

    /// Get a display name for logging purposes
    pub fn display_name(&self) -> String {
        match self {
            DatabaseConfig::Sqlite { path } => format!("SQLite: {:?}", path),
            DatabaseConfig::Postgres { url } => {
                //
                // Hide password in URL for display.
                //
                if let Some(at_pos) = url.find('@') {
                    if let Some(slash_pos) = url[..at_pos].rfind('/') {
                        let prefix = &url[..slash_pos + 1];
                        let suffix = &url[at_pos..];
                        return format!("PostgreSQL: {}***{}", prefix, suffix);
                    }
                }
                format!("PostgreSQL: {}", url)
            }
        }
    }

    /// Check if this is a PostgreSQL configuration
    pub fn is_postgres(&self) -> bool {
        matches!(self, DatabaseConfig::Postgres { .. })
    }

    /// Check if this is a SQLite configuration
    pub fn is_sqlite(&self) -> bool {
        matches!(self, DatabaseConfig::Sqlite { .. })
    }
}
