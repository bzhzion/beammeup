//! Reusable command snippets: simple local storage (a JSON file), independent of the window/pipe,
//! readable and editable even when BeamMeUp isn't running. No secrets here, just command text, so
//! there's no need for the named pipe or elevation for add/list/remove.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    snippets: BTreeMap<String, String>,
}

/// Location of the snippets file, resolved per platform:
/// `%LOCALAPPDATA%\beammeup\` on Windows, `$XDG_CONFIG_HOME/beammeup/` (or `~/.config/beammeup/`)
/// on Linux. The previous version read `LOCALAPPDATA` by hand with a fallback to `"."`: outside
/// Windows that variable doesn't exist, so the file was written **in the current directory**,
/// meaning a different location depending on where the command was launched from (polluting
/// repositories along the way).
fn store_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("beammeup")
        .join("snippets.json")
}

fn load() -> Store {
    let path = store_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(store: &Store) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("write failed: {e}"))
}

pub fn add(name: &str, text: &str) -> Result<(), String> {
    let mut store = load();
    store.snippets.insert(name.to_string(), text.to_string());
    save(&store)
}

pub fn remove(name: &str) -> Result<(), String> {
    let mut store = load();
    if store.snippets.remove(name).is_none() {
        return Err(format!("unknown snippet: {name}"));
    }
    save(&store)
}

pub fn get(name: &str) -> Result<String, String> {
    load()
        .snippets
        .get(name)
        .cloned()
        .ok_or_else(|| format!("unknown snippet: {name}"))
}

pub fn list() -> Vec<(String, String)> {
    load().snippets.into_iter().collect()
}
