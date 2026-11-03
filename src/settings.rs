//! User settings: which AutoHotkey executable to run and which script to give it.
//!
//! Stored in %APPDATA%\orderup\settings.json (override with --settings PATH), written by
//! `orderup setup`.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Settings {
    pub ahk_exe: String,
    pub ahk_script: String,
    /// Where `setup` copied the program to (used by `uninstall`).
    #[serde(default)]
    pub installed_exe: String,
}

pub fn path(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--settings") {
        if let Some(p) = args.get(i + 1) {
            return PathBuf::from(p);
        }
    }
    profile::app_dir().join("settings.json")
}

/// `Ok(None)` if there are no settings yet.
pub fn load(path: &Path) -> Result<Option<Settings>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{} is not a valid settings file: {e}", path.display()))
}

pub fn save(path: &Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("can't write {}: {e}", path.display()))
}

/// Warns (doesn't fail) about paths that have disappeared since setup.
pub fn warn_if_missing(s: &Settings) {
    for (what, p) in [("AutoHotkey executable", &s.ahk_exe), ("AHK script", &s.ahk_script)] {
        if !Path::new(p).is_file() {
            eprintln!("warning: {what} not found: {p} (fix with: orderup setup)");
        }
    }
}

/// Looks for a standard AutoHotkey v2 install to offer as the default.
pub fn detect_ahk() -> Option<String> {
    let roots = ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v))
        .map(PathBuf::from);
    for root in roots {
        let base = if root.ends_with("Local") { root.join("Programs") } else { root };
        for exe in ["AutoHotkey64.exe", "AutoHotkey.exe"] {
            let p = base.join("AutoHotkey").join("v2").join(exe);
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    None
}
