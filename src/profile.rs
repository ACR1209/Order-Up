//! Per-device calibration profiles, stored as JSON.
//!
//! Default location: %APPDATA%\orderup\profiles.json (override with --config PATH), so the
//! always-on binary doesn't depend on the working directory it was launched from.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dsp::BLOCK;

pub const VERSION: u32 = 3;

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub sr: u32,
    pub tpl: Vec<f32>,
    pub peaks: Vec<usize>,
    pub sim_t: f32,
    pub tonal_t: f32,
    pub min_rms: f32,
    #[serde(default)]
    pub peak_hz: Vec<u32>, // informational
    #[serde(default)]
    pub max_level: f32, // informational
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub block: usize,
    pub devices: BTreeMap<String, Profile>,
}

impl Config {
    pub fn new(devices: BTreeMap<String, Profile>) -> Self {
        Self { version: VERSION, block: BLOCK, devices }
    }
}

/// %APPDATA%\orderup, where profiles and settings live.
pub fn app_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join("orderup"),
        None => PathBuf::from("."),
    }
}

/// The program used to be called bell-rs. Carry its saved calibration/settings over once,
/// so renaming doesn't force a recalibration. Does nothing if there's nothing to move or
/// the new folder already exists.
pub fn migrate_legacy_dir() {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else { return };
    let (old, new) = (appdata.join("bell-rs"), appdata.join("orderup"));
    if old.is_dir() && !new.exists() && std::fs::rename(&old, &new).is_ok() {
        println!("Moved your saved settings from {} to {}", old.display(), new.display());
    }
}

pub fn path(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--config") {
        if let Some(p) = args.get(i + 1) {
            return PathBuf::from(p);
        }
    }
    app_dir().join("profiles.json")
}

/// Reads the profiles; `Ok(None)` if the file doesn't exist or is from an incompatible version.
pub fn load(path: &PathBuf) -> Result<Option<Config>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    let cfg: Config = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not a valid profile file: {e}", path.display()))?;
    if cfg.version != VERSION || cfg.block != BLOCK {
        return Ok(None);
    }
    Ok(Some(cfg))
}

pub fn save(path: &PathBuf, cfg: &Config) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string(cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("can't write {}: {e}", path.display()))
}
