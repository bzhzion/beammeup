use serde::{Deserialize, Serialize};

pub use crate::shells::ShellDescriptor;

/// Canonical name of the control named pipe, derived from the username so it never collides
/// between two Windows sessions open on the same machine.
///
/// Windows only: on the Unix side the socket is named from the real UID (see
/// `ipc::unix_impl::socket_path`), not the username, because `$USER` is `root` in the elevated
/// window while the CLI connector still sees the original user. Yet both must designate the same
/// channel.
#[cfg(windows)]
pub fn control_channel_name() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "default".to_string());
    format!("beammeup-{user}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    List,
    /// Lists the interpreters actually detected on THIS machine (PowerShell 5.1/7, cmd, Git Bash,
    /// WSL distributions). Never a fixed list, each machine can have a different set installed in
    /// different locations.
    ListShells,
    Open {
        /// An id returned by `ListShells` (e.g. "pwsh5", "gitbash", "wsl:Debian"). Mutually
        /// exclusive with `ssh_args`.
        shell: Option<String>,
        /// Arguments passed as-is to the system's `ssh` client (user@host, -p, -i, ...). Mutually
        /// exclusive with `shell`/`scp_args`.
        ssh_args: Option<String>,
        /// Arguments passed as-is to the system's `scp` client, for a file transfer visible in a
        /// tab (upload or download depending on the direction of the arguments). Reuses the same
        /// SSH key/agent as `ssh_args`. Mutually exclusive with `shell`/`ssh_args`.
        #[serde(default)]
        scp_args: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
        /// Free-form name chosen by the caller (human or agent) to find this session again later
        /// via `list`/`send`/`read`/... without having to remember the UUID. Useful when several
        /// agents work in parallel on the same machine. No uniqueness enforced: in case of a
        /// duplicate, resolution picks the most recent session with that label.
        label: Option<String>,
    },
    /// `id` accepts either a session's exact UUID or a label set at open time (resolved
    /// server-side, see `SessionManager::resolve`).
    Send {
        id: String,
        text: String,
    },
    /// Sends a command, waits for it to finish (via an end marker injected after the command,
    /// specific to the session's shell, see `SessionManager::exec`), and returns its output and
    /// exit code. Unlike `Send`, blocks until completion or timeout expiry instead of returning
    /// immediately: this is the command-completion detection that was missing, which until now
    /// forced guessing a delay and then reading back.
    Exec {
        id: String,
        command: String,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Key {
        id: String,
        key: String,
    },
    Read {
        id: String,
        since: Option<u64>,
        /// Ignores `since` and returns only what happened since the last `Send`/`Key` on this
        /// session (no prompt detection: just "since I last typed something", good enough for
        /// agent usage most of the time).
        #[serde(default)]
        last: bool,
        /// Strips ANSI sequences from the output before returning it.
        #[serde(default)]
        plain: bool,
    },
    Resize {
        id: String,
        cols: u16,
        rows: u16,
    },
    /// Brings an already open tab to the foreground in the UI, without creating a new one. Useful
    /// for showing a specific session to the human when several are running in parallel; until
    /// now, only `Open` gave focus, with no way to switch back to an existing tab afterwards.
    SelectTab {
        id: String,
    },
    Close {
        id: String,
    },
    /// Closes all sessions without quitting the application (unlike `Quit`).
    CloseAll,
    /// Opens a new session with the same parameters as an existing session.
    DuplicateTab {
        id: String,
    },
    /// Reopens the last closed session (whichever it was), with the same parameters.
    ReopenLastClosed,
    /// Cleanly closes all sessions then quits the application (window included). This is the only
    /// "clean" way to close BeamMeUp from the outside: an elevated process cannot be killed by a
    /// non-elevated process (`Access is denied`), so we never rely on `taskkill`/`Stop-Process`;
    /// instead we ask it to leave via this same API.
    Quit,
    SetFullscreen {
        enabled: bool,
    },
    /// Starts (or reconfigures) the optional remote web server, opt-in and off by default (see
    /// `remote_web.rs`). `bind` and `token` reflect only what the caller actually specified: the
    /// handler resolves unset fields from the persisted `remote.json` config (bind) or by
    /// generating a fresh one (token), then saves the resolved settings back so a bare
    /// `beammeup web on` reuses the last bind address. If `no_token` is `true`, the server starts
    /// with no authentication regardless of any saved token; else if `token` is `Some`, it is used
    /// as-is; else a random token is generated.
    WebOn {
        bind: Option<String>,
        token: Option<String>,
        #[serde(default)]
        no_token: bool,
    },
    /// Stops the remote web server if running. Does nothing if it wasn't.
    WebOff,
    /// Current state of the remote web server (running or not, bind address, whether a token is
    /// configured). Never returns the token value itself.
    WebStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    /// Id of the detected shell (e.g. "pwsh5", "gitbash", "wsl:Debian") or "ssh" for an SSH
    /// session.
    pub kind: String,
    pub title: String,
    pub label: Option<String>,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Error {
        message: String,
    },
    Status {
        window_visible: bool,
        session_count: usize,
    },
    List {
        sessions: Vec<SessionInfo>,
    },
    Shells {
        shells: Vec<ShellDescriptor>,
    },
    Opened {
        id: String,
    },
    Output {
        data: String,
        cursor: u64,
        alive: bool,
    },
    Exec {
        output: String,
        /// `None` if the timeout expired before the end marker appeared (`timed_out` is `true` in
        /// that case). The command may very well still be running.
        exit_code: Option<i32>,
        timed_out: bool,
    },
    Closed {
        count: usize,
    },
    /// Successful `WebOn`: the actually bound address (useful when the caller asked for an
    /// OS-assigned port, e.g. `"127.0.0.1:0"`), and the token in effect if any. Returned once so
    /// the CLI can print it to the user; never persisted anywhere in cleartext beyond this single
    /// response and `remote.json` itself.
    WebStarted {
        bind: String,
        token: Option<String>,
    },
    WebStatus {
        running: bool,
        bind: Option<String>,
        token_set: bool,
    },
}
