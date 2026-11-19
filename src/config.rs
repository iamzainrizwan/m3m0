//! Persisted UI settings: ~/.config/m3m0/config.json.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Config {
    /// None until first run picks one from the monitor size.
    pub zoom: Option<f32>,
    pub sidebar_w: f32,
    pub sidebar_open: bool,
    /// None = auto-detect (largest ~/remnote/*/remnote.db).
    pub db: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config { zoom: None, sidebar_w: 240.0, sidebar_open: true, db: None }
    }
}

fn path() -> PathBuf {
    crate::kb::home().join(".config/m3m0/config.json")
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}
