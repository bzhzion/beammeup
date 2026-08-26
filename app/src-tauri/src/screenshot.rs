//! Capture of a real screenshot of the window via the remote debug port of the Chrome DevTools
//! Protocol (CDP) exposed by WebView2. Exists because the environment the agent runs in is
//! sandboxed and has no access to the Windows screen capture API (GDI). But since the window's
//! content is a web page like any other, it can be captured over the network instead of through
//! the operating system.
//!
//! **Windows only.** The mechanism is tied to the CDP protocol, specific to Chromium engines. On
//! Linux, Tauri uses WebKitGTK, which does not implement CDP and exposes no equivalent debug port:
//! there is therefore nothing to port here, only a different technology to swap in if the need is
//! confirmed. In the meantime, `capture()` returns an explicit error there rather than failing on
//! a misleading "unreachable port" while the window is running perfectly fine.

#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use std::net::TcpStream;
use std::path::Path;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
pub const CDP_PORT: u16 = 9222;

/// On platforms without CDP (Linux/macOS), we explicitly refuse rather than attempt a connection
/// that is bound to fail. `beammeup read --plain` covers the essential real need (knowing what a
/// session displays), without depending on the rendering engine.
#[cfg(not(windows))]
pub fn capture(_out_path: &Path) -> Result<(), String> {
    Err("`screenshot` is only available on Windows: it relies on WebView2's Chrome DevTools debug \
         port, which WebKitGTK (used by Tauri on this platform) does not expose. To read what a \
         session displays, use `beammeup read <id> --plain`."
        .to_string())
}

/// Minimal hand-rolled HTTP request: the CDP debug server is a simple local HTTP server, no need
/// for a full HTTP dependency for a single GET.
#[cfg(windows)]
fn http_get_json(path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", CDP_PORT))
        .map_err(|e| format!("could not connect to debug port ({CDP_PORT}): {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    // No "Connection: close": the CDP debug HTTP server keeps the connection open (keep-alive) no
    // matter what we ask for, confirmed by testing, `read_to_string` (which waits for EOF) blocks
    // until the timeout. So we read `Content-Length` and stop right after.
    // Host MUST include the port: the CDP debug server builds webSocketDebuggerUrl from the
    // received Host (to stay correct behind a possible proxy). Without the port here, it returned
    // "ws://127.0.0.1/devtools/..." (missing port), confirmed by testing.
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{CDP_PORT}\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("failed to send request: {e}"))?;

    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let headers_end = loop {
        let n = stream
            .read(&mut chunk)
            .map_err(|e| format!("failed to read response: {e}"))?;
        if n == 0 {
            return Err("connection closed before the end of the HTTP headers".to_string());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
    };

    let headers = String::from_utf8_lossy(&buf[..headers_end]);
    let content_length: usize = headers
        .lines()
        .find_map(|l| l.to_lowercase().starts_with("content-length:").then(|| l))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse().ok())
        .ok_or("no Content-Length in the HTTP response")?;

    while buf.len() < headers_end + content_length {
        let n = stream
            .read(&mut chunk)
            .map_err(|e| format!("failed to read body: {e}"))?;
        if n == 0 {
            return Err("connection closed before the end of the HTTP body".to_string());
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    let body = &buf[headers_end..headers_end + content_length];
    Ok(String::from_utf8_lossy(body).to_string())
}

#[cfg(windows)]
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Captures the BeamMeUp window (the active tab, as seen by the human) and writes a PNG to
/// `out_path`. Assumes the window is already running (the caller must have made sure it is
/// visible beforehand, e.g. via `Request::Status`, which relaunches the window if needed).
#[cfg(windows)]
pub fn capture(out_path: &Path) -> Result<(), String> {
    let list_json = http_get_json("/json")?;
    let targets: serde_json::Value = serde_json::from_str(&list_json)
        .map_err(|e| format!("unreadable /json response: {e}"))?;
    let page = targets
        .as_array()
        .and_then(|arr| arr.iter().find(|t| t["type"] == "page"))
        .ok_or("no page found on the debug port, is the window actually running?")?;
    // IPv4 explicitly forced (instead of "localhost", which may try ::1 depending on system
    // resolution before falling back to IPv4).
    let ws_url = page["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("no websocket URL in the CDP response")?
        .replace("localhost", "127.0.0.1");

    let (mut socket, _) =
        tungstenite::connect(&ws_url).map_err(|e| format!("CDP websocket connection failed: {e}"))?;

    let request = serde_json::json!({
        "id": 1,
        "method": "Page.captureScreenshot",
        "params": { "format": "png" }
    });
    socket
        .send(tungstenite::Message::Text(request.to_string().into()))
        .map_err(|e| format!("failed to send CDP command: {e}"))?;

    loop {
        let msg = socket
            .read()
            .map_err(|e| format!("failed to read CDP response: {e}"))?;
        let tungstenite::Message::Text(text) = msg else {
            continue;
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("unreadable CDP response: {e}"))?;
        if let Some(data) = value["result"]["data"].as_str() {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| format!("base64 decoding failed: {e}"))?;
            std::fs::write(out_path, bytes)
                .map_err(|e| format!("failed to write file: {e}"))?;
            return Ok(());
        }
        if let Some(err) = value.get("error") {
            return Err(format!("CDP returned an error: {err}"));
        }
    }
}
