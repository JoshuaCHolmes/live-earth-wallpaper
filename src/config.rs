//! Persistent user settings.
//!
//! Stored as JSON at `%LOCALAPPDATA%\LiveEarthWallpaper\config.json`.
//! Missing/corrupt files fall back to defaults silently — settings aren't
//! critical and we don't want to block startup over them.

use crate::monitor::MultiMonitorMode;
use crate::satellite::Satellite;
use crate::wallpaper::{self, WallpaperTarget};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub target: WallpaperTarget,
    #[serde(default)]
    pub mode: MultiMonitorMode,
    #[serde(default = "default_show_earth")]
    pub show_earth: bool,
    #[serde(default)]
    pub show_labels: bool,
    #[serde(default = "default_satellite")]
    pub satellite: Satellite,
}

fn default_show_earth() -> bool { true }
fn default_satellite() -> Satellite { Satellite::GoesEast }

impl Default for Config {
    fn default() -> Self {
        Self {
            target: WallpaperTarget::default(),
            mode: MultiMonitorMode::default(),
            show_earth: true,
            show_labels: false,
            satellite: Satellite::GoesEast,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    wallpaper::wallpaper_dir().ok().map(|d| d.join("config.json"))
}

pub fn load() -> Config {
    let Some(path) = config_path() else { return Config::default() };
    let Ok(bytes) = std::fs::read(&path) else { return Config::default() };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        tracing::warn!("Failed to parse config.json, using defaults: {}", e);
        Config::default()
    })
}

pub fn save(cfg: &Config) {
    let Some(path) = config_path() else { return };
    match serde_json::to_vec_pretty(cfg) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                tracing::warn!("Failed to save config.json: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize config: {}", e),
    }
}
