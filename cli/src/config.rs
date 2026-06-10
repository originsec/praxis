use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const CONFIG_DIR: &str = "praxis";
const CONFIG_FILE: &str = "config";

//
// User-level CLI config. Stored at ~/.config/praxis/config in
// KEY=VALUE form so it lines up with /etc/praxis/env on the
// service side and stays trivial to inspect/edit by hand.
//

pub fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .context("cannot determine config directory")?;
    Ok(base.join(CONFIG_DIR).join(CONFIG_FILE))
}

fn parse(content: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

pub fn load() -> BTreeMap<String, String> {
    config_path()
        .ok()
        .and_then(|p| fs::read_to_string(&p).ok())
        .map(|s| parse(&s))
        .unwrap_or_default()
}

pub fn get(key: &str) -> Option<String> {
    load().get(key).cloned()
}

pub fn set(key: &str, value: &str) -> Result<PathBuf> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut map = if path.exists() {
        parse(&fs::read_to_string(&path)?)
    } else {
        BTreeMap::new()
    };
    map.insert(key.to_string(), value.to_string());
    let mut out = String::new();
    for (k, v) in &map {
        out.push_str(k);
        out.push('=');
        out.push_str(v);
        out.push('\n');
    }
    fs::write(&path, out)?;
    Ok(path)
}

//
// Resolution for the RabbitMQ URL the CLI connects with:
//   1. ~/.config/praxis/config: PRAXIS_RABBITMQ_URL
//   2. compiled-in default
//
// Use `praxis set-rabbitmqurl <url>` to set it.
//

pub fn resolve_rabbitmq_url() -> String {
    get("PRAXIS_RABBITMQ_URL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| common::DEFAULT_RABBITMQ_URL.to_string())
}
