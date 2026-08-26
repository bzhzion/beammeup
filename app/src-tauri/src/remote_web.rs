//! Optional remote web access: a small HTTP server (axum) that lets a phone browser view and
//! control sessions over the network. **Closed by default**, exactly like the rest of BeamMeUp:
//! nothing in this module runs unless the user explicitly starts it (`beammeup web on`, or the
//! persisted `autostart` setting in `remote.json`).
//!
//! Deliberately HTTP polling only, no WebSocket: this keeps the networked code small and
//! auditable, a real safety measure given there is no Rust toolchain available to compile-check
//! this module in the session that wrote it.
//!
//! **The user is fully responsible for the bind address and token they choose.** This module does
//! not restrict or refuse any bind address, including `0.0.0.0`: same philosophy already used for
//! standing elevation (documented risk, not blocked by design, see the README `## Security`
//! section). What this module DOES enforce on its own: a constant-time comparison for the token
//! (never `==` on a secret), and no auth check at all when the user explicitly configured no
//! token.

use std::net::TcpListener as StdTcpListener;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::session::SessionManager;

const REMOTE_WEB_HTML: &str = include_str!("../assets/remote_web.html");

/// Persisted settings, stored next to `snippets.json` (see `snippets.rs`'s `store_path`, the same
/// pattern is mirrored here for `config_path`/`load_config`/`save_config`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub bind: String,
    pub token: Option<String>,
    /// If `true`, the server starts automatically (with these saved settings) when the window
    /// launches, with no need to run `beammeup web on` again after every restart.
    #[serde(default)]
    pub autostart: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:9871".to_string(),
            token: None,
            autostart: false,
        }
    }
}

/// Location of `remote.json`, resolved exactly like `snippets.rs`'s `store_path`:
/// `dirs::config_dir()/beammeup/remote.json`, with the current directory as a last-resort
/// fallback.
fn config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("beammeup")
        .join("remote.json")
}

pub fn load_config() -> RemoteConfig {
    let path = config_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Unlike `snippets.json` (plain command text, not a secret by design), `remote.json` stores a
/// real bearer token once the user has run `beammeup web on`: it is written with owner-only
/// permissions on Linux (0600), otherwise a bare `std::fs::write` would respect the process
/// umask (commonly 022, so world-readable 644), letting any other local account read a token
/// that grants full network access to this machine's admin-elevated shell, inconsistent with the
/// 0600 hardening already applied to the Unix control socket for the exact same threat model
/// (a shared Linux machine, multiple local accounts). Windows is unaffected: `%LOCALAPPDATA%` is
/// already ACL-scoped to the profile that owns it.
pub fn save_config(config: &RemoteConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("write failed: {e}"))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
        // `.mode()` only applies to a fresh file: retighten one written by a pre-fix version.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        return Ok(());
    }
    #[cfg(not(unix))]
    std::fs::write(&path, json).map_err(|e| format!("write failed: {e}"))
}

/// A random token: 32 hex characters from 16 random bytes, no external crypto crate needed for
/// this (a bearer token compared in constant time is enough for this use case).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time byte comparison, required by this repository's `secu-audit` skill for every
/// secret comparison: a plain `==` on the raw strings would short-circuit on the first differing
/// byte, letting an attacker recover the token one byte at a time from response timing.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

struct RunningServer {
    bind: String,
    token_set: bool,
    shutdown: tokio::sync::oneshot::Sender<()>,
}

/// Live handle to the (possibly running) remote web server, held by `WindowControls` so the
/// already-running window process can start/stop it without a restart, exactly like
/// `set_fullscreen` toggles fullscreen live on the running instance.
#[derive(Default)]
pub struct RemoteWebHandle {
    inner: Mutex<Option<RunningServer>>,
}

#[derive(Clone)]
struct AppState {
    manager: Arc<SessionManager>,
    token: Option<String>,
}

impl RemoteWebHandle {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Starts (replacing any previously running instance) the remote web server.
    ///
    /// The TCP listener is bound **synchronously**, before anything else: this way an invalid or
    /// already-used bind address (e.g. `"127.0.0.1:0"` with a typo, or a port already taken)
    /// surfaces immediately as a clear error to the caller (the CLI, or the startup autostart
    /// check), instead of failing silently inside a background task. The actual axum server then
    /// runs as a background task on Tauri's async runtime (not a bare `tokio::spawn`: this
    /// function can be called from `setup()`, before any tokio task is necessarily running, and
    /// `tauri::async_runtime::spawn` is the pattern already used elsewhere in this codebase for
    /// exactly that reason, see `ipc::serve` in `lib.rs`).
    pub fn start(
        &self,
        manager: Arc<SessionManager>,
        bind: String,
        token: Option<String>,
    ) -> Result<String, String> {
        self.stop();

        let std_listener =
            StdTcpListener::bind(&bind).map_err(|e| format!("failed to bind {bind}: {e}"))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| format!("failed to configure the listener: {e}"))?;
        let actual_bind = std_listener
            .local_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| bind.clone());

        let token_set = token.is_some();
        let state = AppState { manager, token };
        let router = build_router(state);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let bind_for_task = actual_bind.clone();
        tauri::async_runtime::spawn(async move {
            // The conversion to a tokio listener happens here (inside the async task, therefore
            // inside a live tokio runtime context) rather than before spawning: `from_std`
            // registers the socket with the reactor, which requires an active runtime.
            let listener = match tokio::net::TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!(
                        "beammeup: remote web server failed to take over the listener on \
                         {bind_for_task} ({e})"
                    );
                    return;
                }
            };
            let server = axum::serve(listener, router).with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            });
            if let Err(e) = server.await {
                eprintln!(
                    "beammeup: remote web server on {bind_for_task} stopped with an error: {e}"
                );
            }
        });

        *self.inner.lock().unwrap() = Some(RunningServer {
            bind: actual_bind.clone(),
            token_set,
            shutdown: shutdown_tx,
        });

        Ok(actual_bind)
    }

    /// Stops the server if running. Returns whether something was actually stopped.
    pub fn stop(&self) -> bool {
        if let Some(running) = self.inner.lock().unwrap().take() {
            let _ = running.shutdown.send(());
            true
        } else {
            false
        }
    }

    /// `(is_running, bind_address_if_running, token_is_set)`.
    pub fn status(&self) -> (bool, Option<String>, bool) {
        match self.inner.lock().unwrap().as_ref() {
            Some(running) => (true, Some(running.bind.clone()), running.token_set),
            None => (false, None, false),
        }
    }
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_page))
        .route("/api/sessions", get(api_sessions))
        .route("/api/sessions/:id/read", get(api_read))
        .route("/api/sessions/:id/send", post(api_send))
        .with_state(state)
        .layer(axum::middleware::from_fn(add_security_headers))
}

/// `X-Content-Type-Options`/`Cache-Control` on every response (terminal output must never be
/// cached by an intermediary on a plain-HTTP LAN), `frame-ancestors 'none'` so a hostile local
/// page cannot iframe this page and clickjack a phone into acting on it. No CSP beyond that: the
/// page is a single self-contained document with no third-party resource and no user-supplied
/// HTML (the client only ever uses `textContent`, never `innerHTML`), so there is nothing else
/// here for a stricter policy to usefully restrict.
async fn add_security_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static("frame-ancestors 'none'"),
    );
    res
}

async fn serve_page() -> Html<&'static str> {
    Html(REMOTE_WEB_HTML)
}

/// Only meaningful when no token is configured (`--no-token`): a browser that has been DNS-rebound
/// onto this server (a hostile page's own domain resolved to a 1-second-TTL record that later
/// repoints at this bind address) sends the attacker's hostname in `Host`, not an IP literal or
/// `localhost`. Restricting to those closes that vector at zero cost on the normal, token-protected
/// path (this check is never reached once a token is set): an IP literal or `localhost` cannot be
/// DNS-rebound to, since there is no name resolution step to poison. A Tailscale MagicDNS hostname
/// would fail this check too, but it is only reachable by someone already inside the tailnet, and
/// setting a token (the documented recommendation for any non-loopback bind) sidesteps this
/// entirely regardless.
fn host_is_rebinding_safe(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let host = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host == "localhost" || host.parse::<std::net::IpAddr>().is_ok()
}

/// Checks the `Authorization: Bearer <token>` header, falling back to a `?token=` query parameter
/// (a phone browser's plain navigation to `/` cannot set custom headers, only JS-driven `fetch`
/// calls after the page has loaded can). If no token is configured at all, every request is
/// accepted as long as the `Host` header rules out DNS rebinding: an explicit user choice (they
/// disabled auth), not an oversight, but one that must not also be exploitable by merely visiting
/// an unrelated web page.
fn check_auth(state: &AppState, headers: &HeaderMap, query_token: Option<&str>) -> bool {
    let Some(expected) = state.token.as_deref() else {
        return host_is_rebinding_safe(headers);
    };
    let from_header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if let Some(supplied) = from_header {
        if constant_time_eq(supplied, expected) {
            return true;
        }
    }
    if let Some(supplied) = query_token {
        if constant_time_eq(supplied, expected) {
            return true;
        }
    }
    false
}

fn unauthorized() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
        .into_response()
}

#[derive(Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

async fn api_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQuery>,
) -> axum::response::Response {
    if !check_auth(&state, &headers, auth.token.as_deref()) {
        return unauthorized();
    }
    Json(state.manager.list()).into_response()
}

#[derive(Deserialize)]
struct ReadQuery {
    since: Option<u64>,
    #[serde(default)]
    plain: bool,
    token: Option<String>,
}

async fn api_read(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<ReadQuery>,
) -> axum::response::Response {
    if !check_auth(&state, &headers, query.token.as_deref()) {
        return unauthorized();
    }
    match state
        .manager
        .read(&id, query.since.unwrap_or(0), false, query.plain)
    {
        Ok((data, cursor, alive)) => {
            Json(json!({"data": data, "cursor": cursor, "alive": alive})).into_response()
        }
        Err(message) => (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response(),
    }
}

#[derive(Deserialize)]
struct SendQuery {
    token: Option<String>,
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
}

async fn api_send(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<SendQuery>,
    Json(body): Json<SendBody>,
) -> axum::response::Response {
    if !check_auth(&state, &headers, query.token.as_deref()) {
        return unauthorized();
    }
    match state.manager.send_input(&id, &body.text) {
        Ok(()) => Json(json!({})).into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response(),
    }
}
