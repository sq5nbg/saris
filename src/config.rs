//! Application configuration loaded from a TOML file (`saris.toml`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Allowed document types (fixed by the specification).
pub const DOC_TYPES: [&str; 2] = ["raport", "sprawozdanie"];

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub admin: AdminConfig,
    #[serde(default)]
    pub smtp: SmtpConfig,
    #[serde(default)]
    pub subjects: Vec<Subject>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_storage_dir")]
    pub storage_dir: PathBuf,
    #[serde(default = "default_log_file")]
    pub log_file: PathBuf,
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: u64,
    #[serde(default = "default_allowed_extensions")]
    pub allowed_extensions: Vec<String>,
    #[serde(default)]
    pub trust_proxy: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdminConfig {
    pub username: String,
    pub password: String,
}

/// TLS mode used for the SMTP connection.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SmtpTls {
    /// Implicit TLS on port 465, STARTTLS otherwise.
    #[default]
    Auto,
    /// Implicit TLS (SMTPS).
    Tls,
    /// Mandatory STARTTLS.
    Starttls,
    /// Plain-text connection (only for local test servers).
    None,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub from: String,
    /// Optional extension of the spec: TLS mode (auto | tls | starttls | none).
    #[serde(default)]
    pub tls: SmtpTls,
}

impl Default for SmtpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: default_smtp_port(),
            username: String::new(),
            password: String::new(),
            from: String::new(),
            tls: SmtpTls::Auto,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Subject {
    pub name: String,
    pub password: String,
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_string()
}
fn default_storage_dir() -> PathBuf {
    PathBuf::from("pliki")
}
fn default_log_file() -> PathBuf {
    PathBuf::from("logs/saris.log")
}
fn default_max_file_size_mb() -> u64 {
    20
}
fn default_allowed_extensions() -> Vec<String> {
    vec!["pdf".to_string(), "ipynb".to_string()]
}
fn default_smtp_port() -> u16 {
    587
}

impl Config {
    /// Read and validate the configuration file.
    pub fn load(path: &Path) -> anyhow_like::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read config file {}: {e}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).map_err(|e| format!("invalid config {}: {e}", path.display()))?;
        cfg.normalize();
        cfg.validate()?;
        Ok(cfg)
    }

    fn normalize(&mut self) {
        self.server.allowed_extensions = self
            .server
            .allowed_extensions
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        for s in &mut self.subjects {
            s.name = s.name.trim().to_string();
        }
    }

    fn validate(&self) -> anyhow_like::Result<()> {
        if self.server.max_file_size_mb == 0 {
            return Err("server.max_file_size_mb must be greater than 0".into());
        }
        if self.server.allowed_extensions.is_empty() {
            return Err("server.allowed_extensions must not be empty".into());
        }
        if self.subjects.is_empty() {
            return Err("at least one [[subjects]] entry is required".into());
        }
        let mut seen = HashSet::new();
        for s in &self.subjects {
            if s.name.is_empty() {
                return Err("subject name must not be empty".into());
            }
            if !crate::storage::is_safe_segment(&s.name) {
                return Err(format!(
                    "subject name {:?} may contain only letters, digits, '_', '-' and '.'",
                    s.name
                ));
            }
            if !seen.insert(s.name.to_lowercase()) {
                return Err(format!("duplicate subject name {:?}", s.name));
            }
        }
        if self.smtp.enabled && (self.smtp.host.is_empty() || self.smtp.from.is_empty()) {
            return Err("smtp.host and smtp.from are required when smtp.enabled = true".into());
        }
        Ok(())
    }

    /// Maximum file size in bytes.
    pub fn max_file_bytes(&self) -> u64 {
        self.server.max_file_size_mb * 1024 * 1024
    }

    pub fn find_subject(&self, name: &str) -> Option<&Subject> {
        self.subjects.iter().find(|s| s.name == name)
    }
}

/// Minimal string-based error alias to avoid an extra dependency.
pub mod anyhow_like {
    pub type Result<T> = std::result::Result<T, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_config() {
        let text = include_str!("../saris.toml.example");
        let mut cfg: Config = toml::from_str(text).unwrap();
        cfg.normalize();
        cfg.validate().unwrap();
        assert_eq!(cfg.server.max_file_size_mb, 20);
        assert!(cfg.find_subject("PE").is_some());
        assert_eq!(cfg.smtp.tls, SmtpTls::Auto);
    }
}
