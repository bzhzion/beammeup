use tauri::State;

use crate::protocol::SessionInfo;
use crate::session::SessionManager;
use crate::shells::ShellDescriptor;

#[tauri::command]
pub fn ui_list(manager: State<'_, std::sync::Arc<SessionManager>>) -> Vec<SessionInfo> {
    manager.list()
}

#[tauri::command]
pub fn ui_shells() -> Vec<ShellDescriptor> {
    crate::shells::discover()
}

/// Version of the executable actually running (patched by CI at release build time,
/// `0.1.0` otherwise in development). The window title set from the Rust side
/// (`window.set_title`) gets overwritten by the static `<title>` tag in the HTML once the page
/// has loaded (WebView2 resynchronizes the title bar to `document.title`), so the frontend must
/// be the one that sets the final title, via this command.
#[tauri::command]
pub fn ui_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn ui_open(
    manager: State<'_, std::sync::Arc<SessionManager>>,
    shell: Option<String>,
    ssh: Option<String>,
    label: Option<String>,
) -> Result<String, String> {
    manager.open(shell, ssh, None, None, None, label)
}

#[tauri::command]
pub fn ui_send(
    manager: State<'_, std::sync::Arc<SessionManager>>,
    id: String,
    text: String,
) -> Result<(), String> {
    manager.send_input(&id, &text)
}

#[tauri::command]
pub fn ui_key(
    manager: State<'_, std::sync::Arc<SessionManager>>,
    id: String,
    key: String,
) -> Result<(), String> {
    manager.send_key(&id, &key)
}

#[tauri::command]
pub fn ui_resize(
    manager: State<'_, std::sync::Arc<SessionManager>>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    manager.resize(&id, cols, rows)
}

#[tauri::command]
pub fn ui_close(
    manager: State<'_, std::sync::Arc<SessionManager>>,
    id: String,
) -> Result<(), String> {
    manager.close(&id)
}

#[tauri::command]
pub fn ui_read(
    manager: State<'_, std::sync::Arc<SessionManager>>,
    id: String,
    since: Option<u64>,
) -> Result<(String, u64, bool), String> {
    manager.read(&id, since.unwrap_or(0), false, false)
}

#[tauri::command]
pub fn ui_set_fullscreen(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    window.set_fullscreen(enabled).map_err(|e| e.to_string())
}

// Snippets: so far only drivable via `beammeup snippet add/list/run/remove` in the CLI,
// no interface for the human in the window itself. `run` is not part of these
// commands: the frontend fetches the text via `ui_snippet_list` and reuses `ui_send`
// as usual, no need for an extra round trip.
#[tauri::command]
pub fn ui_snippet_list() -> Vec<(String, String)> {
    crate::snippets::list()
}

#[tauri::command]
pub fn ui_snippet_add(name: String, text: String) -> Result<(), String> {
    crate::snippets::add(&name, &text)
}

#[tauri::command]
pub fn ui_snippet_remove(name: String) -> Result<(), String> {
    crate::snippets::remove(&name)
}

