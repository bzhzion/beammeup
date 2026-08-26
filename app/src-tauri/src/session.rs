use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use uuid::Uuid;

use crate::protocol::SessionInfo;
use crate::shells;

const RING_BUFFER_CAP: usize = 512 * 1024;
const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 32;

struct RingBuffer {
    data: VecDeque<u8>,
    /// Total number of bytes ever written to this buffer (monotonic cursor), including those
    /// already evicted by truncation at `RING_BUFFER_CAP`.
    total_written: u64,
}

impl RingBuffer {
    fn new() -> Self {
        Self {
            data: VecDeque::new(),
            total_written: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.data.extend(chunk.iter().copied());
        self.total_written += chunk.len() as u64;
        while self.data.len() > RING_BUFFER_CAP {
            self.data.pop_front();
        }
    }

    /// Returns the bytes written since `since` (absolute cursor), along with the new cursor.
    /// If `since` is too old (already evicted from the ring buffer), returns everything that's left.
    fn since(&self, since: u64) -> (Vec<u8>, u64) {
        let evicted = self.total_written.saturating_sub(self.data.len() as u64);
        let skip = since.saturating_sub(evicted).min(self.data.len() as u64) as usize;
        let out: Vec<u8> = self.data.iter().skip(skip).copied().collect();
        (out, self.total_written)
    }

    fn cursor(&self) -> u64 {
        self.total_written
    }
}

/// Where a session comes from, so an identical one can be reopened (`duplicate`/`reopen_last_closed`)
/// without having to reparse `title` (which only exists for display purposes, e.g. "ssh user@host").
#[derive(Clone)]
pub enum SessionOrigin {
    Shell(String),
    Ssh(String),
    Scp(String),
}

impl SessionOrigin {
    fn into_open_args(self) -> (Option<String>, Option<String>, Option<String>) {
        match self {
            SessionOrigin::Shell(id) => (Some(id), None, None),
            SessionOrigin::Ssh(args) => (None, Some(args), None),
            SessionOrigin::Scp(args) => (None, None, Some(args)),
        }
    }
}

pub struct Session {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub label: Option<String>,
    pub origin: SessionOrigin,
    /// Opening order, used to break ties between several sessions sharing the same label during
    /// resolution (the most recent one wins).
    seq: u64,
    buffer: Arc<Mutex<RingBuffer>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    alive: Arc<Mutex<bool>>,
    /// Ring buffer cursor at the time of the last `send`/`key` on this session. Lets
    /// `read --last` return only what happened since then, without having to detect prompts
    /// (fragile and different for each shell). Simple and robust: "since I last typed something"
    /// is what the agent needs most of the time.
    last_send_cursor: Mutex<u64>,
    /// Minimal entry gate: never write to the PTY before the process has produced its own very
    /// first real output. Borrowed from node-pty (the library behind Tabby and VS Code,
    /// `src/windowsTerminal.ts`, `_isReady`/`_deferreds`).
    ///
    /// **Partial protection, accepted as such.** It only protects the very start of the session;
    /// the occasional loss of the first character of a keystroke arriving after a period of
    /// silence remains a known defect (see `write_raw`). Trying to compensate for it with a
    /// sacrificial byte corrupted the line being edited by PSReadLine in a real-world test on
    /// 2026-08-25 (permanently abandoned): an occasional, visible dropped character is better
    /// than silent corruption of the typed text.
    #[allow(dead_code)]
    a_recu_sortie: Mutex<bool>,
    #[allow(dead_code)]
    a_recu_sortie_cv: Condvar,
}

#[derive(Clone, Serialize)]
pub struct DataEvent {
    pub id: String,
    pub chunk: String,
}

/// Called on every output chunk received from a session, so the GUI window (if running) can push
/// the live stream to the frontend. In pure CLI connector mode (no window initialized yet), this
/// callback is a no-op.
pub type EmitFn = Arc<dyn Fn(DataEvent) + Send + Sync>;
/// Called once, right after a session is created, regardless of its origin (UI button or CLI
/// command via the pipe): lets the frontend bring up the corresponding tab even when it's me (the
/// agent) who opened it from the outside.
pub type OpenedFn = Arc<dyn Fn(SessionInfo) + Send + Sync>;
/// Called once a session has actually been removed from the manager (`close`, regardless of
/// whether it originated from the UI or the CLI): lets the frontend remove the corresponding tab.
/// Before this event was added, a tab closed via `beammeup close` from the agent stayed displayed
/// indefinitely on the UI side (only the backend knew the session was gone): with many sessions
/// opened and closed over the course of a day, the tab bar just kept accumulating dead entries,
/// making navigation unreadable.
pub type ClosedFn = Arc<dyn Fn(String) + Send + Sync>;
/// Called on `SelectTab`: brings an already-open tab to the foreground on the UI side, without
/// creating a new one (unlike `OpenedFn`). Useful for showing a specific session to the human
/// when several are running in parallel.
pub type SelectedFn = Arc<dyn Fn(String) + Send + Sync>;

pub struct SessionManager {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    emit: Mutex<Option<EmitFn>>,
    emit_opened: Mutex<Option<OpenedFn>>,
    emit_closed: Mutex<Option<ClosedFn>>,
    emit_selected: Mutex<Option<SelectedFn>>,
    next_seq: std::sync::atomic::AtomicU64,
    /// Origin of the last closed session (any of them, not necessarily the most recently
    /// opened one): `reopen_last_closed` uses it to reopen it identically. `None` as long as no
    /// session has been closed yet in this window.
    last_closed_origin: Mutex<Option<SessionOrigin>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            emit: Mutex::new(None),
            emit_opened: Mutex::new(None),
            emit_closed: Mutex::new(None),
            emit_selected: Mutex::new(None),
            next_seq: std::sync::atomic::AtomicU64::new(0),
            last_closed_origin: Mutex::new(None),
        }
    }

    pub fn set_opened_emitter(&self, emit: OpenedFn) {
        *self.emit_opened.lock().unwrap() = Some(emit);
    }

    pub fn set_closed_emitter(&self, emit: ClosedFn) {
        *self.emit_closed.lock().unwrap() = Some(emit);
    }

    pub fn set_selected_emitter(&self, emit: SelectedFn) {
        *self.emit_selected.lock().unwrap() = Some(emit);
    }

    /// Resolves `id_or_label` and brings that tab to the foreground on the UI side. Does nothing
    /// more: `show_and_focus` (already called for every request received on the pipe) takes care
    /// of raising the window itself if needed.
    pub fn select(&self, id_or_label: &str) -> Result<(), String> {
        let session = self.get(id_or_label)?;
        if let Some(emit_selected) = self.emit_selected.lock().unwrap().as_ref() {
            emit_selected(session.id.clone());
        }
        Ok(())
    }

    pub fn set_emitter(&self, emit: EmitFn) {
        *self.emit.lock().unwrap() = Some(emit);
    }

    pub fn open(
        &self,
        shell: Option<String>,
        ssh_args: Option<String>,
        scp_args: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
        label: Option<String>,
    ) -> Result<String, String> {
        let id = Uuid::new_v4().to_string();
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let pty_system = native_pty_system();
        let size = PtySize {
            rows: rows.unwrap_or(DEFAULT_ROWS),
            cols: cols.unwrap_or(DEFAULT_COLS),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system
            .openpty(size)
            .map_err(|e| format!("failed to open PTY: {e}"))?;

        #[allow(unused_mut)]
        let (mut cmd, title, kind, origin) = match (shell, ssh_args, scp_args) {
            (Some(shell_id), None, None) => {
                let descriptor = shells::resolve(&shell_id)?;
                let mut builder = CommandBuilder::new(&descriptor.program);
                for arg in &descriptor.args {
                    builder.arg(arg);
                }
                let origin = SessionOrigin::Shell(descriptor.id.clone());
                (builder, descriptor.label, descriptor.id, origin)
            }
            (None, Some(raw_args), None) => {
                let mut builder = CommandBuilder::new("ssh");
                for part in raw_args.split_whitespace() {
                    builder.arg(part);
                }
                (
                    builder,
                    format!("ssh {raw_args}"),
                    "ssh".to_string(),
                    SessionOrigin::Ssh(raw_args),
                )
            }
            (None, None, Some(raw_args)) => {
                let mut builder = CommandBuilder::new("scp");
                for part in raw_args.split_whitespace() {
                    builder.arg(part);
                }
                (
                    builder,
                    format!("scp {raw_args}"),
                    "scp".to_string(),
                    SessionOrigin::Scp(raw_args),
                )
            }
            (None, None, None) => {
                return Err(
                    "requires --shell <id> (see `beammeup shells`), --ssh \"<args>\" or --scp \
                     \"<args>\""
                        .to_string(),
                )
            }
            _ => {
                return Err(
                    "--shell, --ssh and --scp are mutually exclusive".to_string(),
                )
            }
        };

        // On Unix, nothing sets `TERM` for us: a process launched in a PTY inherits the parent's
        // environment, and the BeamMeUp window itself has no `TERM`. Without this, full-screen
        // programs (vim, htop, less...) refuse to start or render in degraded mode. ConPTY takes
        // care of this on its own on Windows, hence why this hasn't been needed there so far.
        #[cfg(unix)]
        {
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("failed to launch process: {e}"))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("failed to clone PTY reader: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("failed to get PTY writer: {e}"))?;

        let buffer = Arc::new(Mutex::new(RingBuffer::new()));
        let alive = Arc::new(Mutex::new(true));
        // The loss of the first character on Windows is handled by `a_recu_sortie`/`write_raw`,
        // see their documentation: nothing to do here at session creation.
        let writer = Arc::new(Mutex::new(writer));

        let session = Arc::new(Session {
            id: id.clone(),
            kind: kind.clone(),
            title: title.clone(),
            label: label.clone(),
            origin,
            seq,
            buffer: buffer.clone(),
            writer: writer.clone(),
            master: Mutex::new(pair.master),
            alive: alive.clone(),
            last_send_cursor: Mutex::new(0),
            a_recu_sortie: Mutex::new(false),
            a_recu_sortie_cv: Condvar::new(),
        });

        self.sessions
            .lock()
            .unwrap()
            .insert(id.clone(), session.clone());

        // Blocking read thread (portable-pty is not async): continuously reads the PTY output,
        // appends it to the ring buffer, and pushes an event to the frontend if one is already
        // attached.
        //
        // It also answers the most common terminal queries itself (e.g. the "cursor position
        // report" ESC[6n that PSReadLine sends on startup): a real terminal (conhost, Windows
        // Terminal, xterm.js) always answers it. If this were left entirely to the frontend, a
        // session opened from the CLI would stay stuck indefinitely as long as no window had a
        // chance to process the stream. This was observed in manual testing.
        let emit_slot = self.emit.lock().unwrap().clone();
        let thread_id = id.clone();
        let responder_writer = writer.clone();
        #[cfg(windows)]
        let reader_session = session.clone();
        let mut child = child;
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        buffer.lock().unwrap().push(chunk);
                        // Entry gate for `write_raw`, see the `a_recu_sortie` field: the very
                        // first real output of the process opens the gate permanently and wakes
                        // up any writes that were queued behind it.
                        #[cfg(windows)]
                        {
                            let mut ouverte = reader_session.a_recu_sortie.lock().unwrap();
                            if !*ouverte {
                                *ouverte = true;
                                reader_session.a_recu_sortie_cv.notify_all();
                            }
                        }
                        auto_respond_terminal_queries(chunk, &responder_writer);
                        if let Some(emit) = &emit_slot {
                            emit(DataEvent {
                                id: thread_id.clone(),
                                chunk: String::from_utf8_lossy(chunk).to_string(),
                            });
                        }
                    }
                    Err(_) => break,
                }
            }
            *alive.lock().unwrap() = false;
            let _ = child.wait();
        });

        if let Some(emit_opened) = self.emit_opened.lock().unwrap().as_ref() {
            emit_opened(SessionInfo {
                id: id.clone(),
                kind,
                title: self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&id)
                    .map(|s| s.title.clone())
                    .unwrap_or_default(),
                label,
                alive: true,
            });
        }

        Ok(id)
    }

    /// Resolves an identifier supplied by the caller (CLI or UI), which can be either the exact
    /// UUID of a session or a label set at opening time. This lets an agent find "its" session
    /// again by a chosen name (e.g. `claude-hae-app-eas`) without having to remember the UUID
    /// from one conversation to the next, including when several agents/Claude sessions are
    /// working in parallel on the same machine. In case of a duplicate label, the most recent
    /// session (highest `seq`) wins.
    fn resolve(&self, id_or_label: &str) -> Result<Arc<Session>, String> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.get(id_or_label) {
            return Ok(s.clone());
        }
        sessions
            .values()
            .filter(|s| s.label.as_deref() == Some(id_or_label))
            .max_by_key(|s| s.seq)
            .cloned()
            .ok_or_else(|| format!("unknown session (neither UUID nor label): {id_or_label}"))
    }

    pub fn send_input(&self, id: &str, text: &str) -> Result<(), String> {
        let session = self.get(id)?;
        *session.last_send_cursor.lock().unwrap() = session.buffer.lock().unwrap().cursor();
        Self::write_raw(&session, text.as_bytes())
    }

    /// Special keys: ctrl-c, ctrl-d, enter, tab, esc...
    pub fn send_key(&self, id: &str, key: &str) -> Result<(), String> {
        let bytes: &[u8] = match key.to_lowercase().as_str() {
            "ctrl-c" => &[0x03],
            "ctrl-d" => &[0x04],
            "ctrl-z" => &[0x1a],
            "enter" => b"\r",
            "tab" => b"\t",
            "esc" | "escape" => &[0x1b],
            other => return Err(format!("unknown key: {other}")),
        };
        let session = self.get(id)?;
        *session.last_send_cursor.lock().unwrap() = session.buffer.lock().unwrap().cursor();
        Self::write_raw(&session, bytes)
    }

    /// Sends a command, waits for it to finish, and returns its output and exit code.
    ///
    /// **Approach.** Command completion is not detected by recognizing a prompt (fragile and
    /// different for each shell): the shell itself is asked to print a unique marker followed by
    /// its exit code right after running the command, and we wait for that marker to appear in
    /// the buffer. This is what Tabby does (`exec_command`), which BeamMeUp was missing until
    /// now: without it, the agent had to guess a delay and then read back.
    ///
    /// Blocking (polling with `std::thread::sleep`): to be called from a context that can afford
    /// to wait (the CLI connector on the agent side, or `spawn_blocking` on the IPC server side
    /// so as not to freeze the tokio runtime).
    pub fn exec(
        &self,
        id: &str,
        command: &str,
        timeout_ms: u64,
    ) -> Result<(String, Option<i32>, bool), String> {
        use std::time::{Duration, Instant};

        let session = self.get(id)?;
        let marker = format!("BMU{}", &Uuid::new_v4().simple().to_string()[..8]);

        // Syntax specific to each shell to print the marker followed by the exit code right
        // after the command, without relying on a `;`/`&&` which doesn't have the same semantics
        // everywhere (PowerShell runs what follows `;` even on failure, cmd.exe wants `&`).
        let kind = session.kind.as_str();
        let wrapped = if kind.starts_with("pwsh") {
            format!("{command}; Write-Output \"{marker}:$LASTEXITCODE\"")
        } else if kind == "cmd" {
            format!("{command} & echo {marker}:%errorlevel%")
        } else {
            // gitbash, wsl:*, ssh: assumed to be a POSIX shell (bash/zsh/sh), which is by far the
            // most common case for these session types. Accepted limitation: an ssh session to a
            // non-POSIX shell (e.g. a remote cmd) will never match the marker and will time out.
            format!("{command}; echo \"{marker}:$?\"")
        };

        // Defensive Ctrl+C before sending the command: if a session got stuck in multi-line input
        // (the ">>" prompt, e.g. from a quote left unclosed by the loss of the first character of
        // a previous `exec` on this same session, see `write_raw`), the wrapped command would
        // otherwise pile up indefinitely on that line without ever running, and every subsequent
        // `exec` on this session would fail with a silent timeout. Has no effect on a session
        // already at a normal prompt.
        Self::write_raw(&session, &[0x03])?;
        std::thread::sleep(std::time::Duration::from_millis(150));

        let start_cursor = session.buffer.lock().unwrap().cursor();
        *session.last_send_cursor.lock().unwrap() = start_cursor;
        Self::write_raw(&session, wrapped.as_bytes())?;
        Self::write_raw(&session, b"\r")?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let marqueur_prefixe = format!("{marker}:");
        loop {
            std::thread::sleep(Duration::from_millis(120));
            let (bytes, _) = session.buffer.lock().unwrap().since(start_cursor);
            let plain = strip_ansi_escapes::strip(&bytes);
            let texte = String::from_utf8_lossy(&plain).to_string();

            // The marker first appears verbatim in the echo of the typed line (even before it
            // runs: PSReadLine echoes back what it's sent), with
            // `$LASTEXITCODE`/`%errorlevel%`/`$?` still unsubstituted right after it. Don't stop
            // there. We start from the end of the text to land directly on the most recent
            // occurrence, instead of re-scanning all of them from the start on every loop.
            //
            // Two forms are accepted as "actually executed": a number (the normal case), or
            // nothing at all up to the end of the line (`$LASTEXITCODE` is `$null` after a
            // command that isn't found, e.g. when the very first character was dropped in the
            // sense of `write_raw`, breaking the wrapped command, and interpolates as an empty
            // string). Only an occurrence followed by something else (the literal `$`/`%` of the
            // unsubstituted variable) is rejected.
            let trouve = texte.rmatch_indices(&marqueur_prefixe).find_map(|(pos, _)| {
                let apres = &texte[pos + marqueur_prefixe.len()..];
                let code_str: String = apres
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                match apres.chars().next() {
                    None | Some('\r') | Some('\n') => Some((pos, None)),
                    _ => code_str.parse::<i32>().ok().map(|code| (pos, Some(code))),
                }
            });
            if let Some((pos, exit_code)) = trouve {
                // The useful output stops before the marker line (which itself contains the
                // wrapped command echoed back, of no interest to the caller).
                let sortie = texte[..pos].to_string();
                return Ok((sortie, exit_code, false));
            }
            let alive = *session.alive.lock().unwrap();
            if !alive || Instant::now() >= deadline {
                return Ok((texte, None, true));
            }
        }
    }

    /// Writes to the PTY. On Windows, holds back any write that happens before the process's
    /// first real output (see `a_recu_sortie`).
    ///
    /// **Abandonment of priming via a sacrificial byte (2026-08-25).** Six successive fixes had
    /// tried to compensate for the loss of the first character by sending a throwaway byte (space
    /// or backspace, once at creation and then in a loop whenever silence was detected). The last
    /// variant (a space followed by a corrective backspace as soon as the echo was confirmed)
    /// **corrupted the line being edited**: observed in a real-world test, typing "bonjour" on the
    /// keyboard with natural pauses produced completely garbled text
    /// ("bobonbonjbonjobonjoubonjourbonjour..."). PSReadLine doesn't just passively echo back
    /// every byte it receives: it maintains its own edit buffer and redraws it on every change, so
    /// any synthetic byte injected alongside a real keystroke risks altering that buffer itself,
    /// not just its display. This goes well beyond the already-known risk of a backspace on an
    /// empty line (PSReadLine#422). There is no safe way to write unsolicited bytes into a PTY
    /// whose line editor on the other end we don't control. The only guard kept is therefore the
    /// one from node-pty (`a_recu_sortie`, with no side effect since it never writes anything);
    /// the occasional loss of the very first character of a session remains a known, unresolved
    /// defect, rather than a risk taken to "fix" it with an unsolicited write into the stream.
    fn write_raw(session: &Session, bytes: &[u8]) -> Result<(), String> {
        #[cfg(windows)]
        {
            let mut ouverte = session.a_recu_sortie.lock().unwrap();
            while !*ouverte {
                ouverte = session.a_recu_sortie_cv.wait(ouverte).unwrap();
            }
        }

        session
            .writer
            .lock()
            .unwrap()
            .write_all(bytes)
            .map_err(|e| format!("write failed: {e}"))
    }

    pub fn read(
        &self,
        id: &str,
        since: u64,
        last: bool,
        plain: bool,
    ) -> Result<(String, u64, bool), String> {
        let session = self.get(id)?;
        let effective_since = if last {
            *session.last_send_cursor.lock().unwrap()
        } else {
            since
        };
        let (bytes, cursor) = session.buffer.lock().unwrap().since(effective_since);
        let alive = *session.alive.lock().unwrap();
        let bytes = if plain {
            strip_ansi_escapes::strip(&bytes)
        } else {
            bytes
        };
        Ok((String::from_utf8_lossy(&bytes).to_string(), cursor, alive))
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self.get(id)?;
        let result = session
            .master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize failed: {e}"));
        result
    }

    pub fn close(&self, id_or_label: &str) -> Result<(), String> {
        let real_id = self.resolve(id_or_label)?.id.clone();
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(&real_id)
            .ok_or_else(|| format!("unknown session: {id_or_label}"))?;
        *session.alive.lock().unwrap() = false;
        *self.last_closed_origin.lock().unwrap() = Some(session.origin.clone());
        // Closing the writer/master is enough to make the child process exit (EOF / hangup) in
        // the vast majority of cases (both powershell.exe and ssh.exe react to a closed pipe);
        // the reader thread terminates on its own on Ok(0) or Err.
        drop(session);
        if let Some(emit_closed) = self.emit_closed.lock().unwrap().as_ref() {
            emit_closed(real_id);
        }
        Ok(())
    }

    /// Opens a new session with the same parameters as an existing one (same shell, or same
    /// ssh/scp arguments). Its label is not copied, to avoid any resolution ambiguity with the
    /// original.
    pub fn duplicate(&self, id_or_label: &str) -> Result<String, String> {
        let session = self.get(id_or_label)?;
        let (shell, ssh_args, scp_args) = session.origin.clone().into_open_args();
        self.open(shell, ssh_args, scp_args, None, None, None)
    }

    /// Reopens the last closed session (any of them, not necessarily the most recently opened
    /// one), with the same parameters.
    pub fn reopen_last_closed(&self) -> Result<String, String> {
        let origin = self
            .last_closed_origin
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "no closed session to reopen".to_string())?;
        let (shell, ssh_args, scp_args) = origin.into_open_args();
        self.open(shell, ssh_args, scp_args, None, None, None)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                kind: s.kind.clone(),
                title: s.title.clone(),
                label: s.label.clone(),
                alive: *s.alive.lock().unwrap(),
            })
            .collect()
    }

    pub fn count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    /// Closes all sessions. Used by `Request::Quit` before quitting the application, and by
    /// `Request::CloseAll` (keep the window open, just clear the tabs). Hence the return value,
    /// useful for this second case but ignored by the first.
    pub fn close_all(&self) -> usize {
        let ids: Vec<String> = self.sessions.lock().unwrap().keys().cloned().collect();
        let mut count = 0;
        for id in ids {
            if self.close(&id).is_ok() {
                count += 1;
            }
        }
        count
    }

    fn get(&self, id_or_label: &str) -> Result<Arc<Session>, String> {
        self.resolve(id_or_label)
    }
}

/// Answers the terminal queries that most shells send on startup or while rendering their
/// prompt, regardless of whether a real terminal (xterm.js or otherwise) is present on the UI
/// side. A fake but stable cursor position (1;1): enough to unblock PSReadLine and the like,
/// which only use it as a capability probe, not as an actually used value.
fn auto_respond_terminal_queries(chunk: &[u8], writer: &Arc<Mutex<Box<dyn Write + Send>>>) {
    // ESC [ 6 n: Device Status Report - Cursor Position Report.
    const DSR_CURSOR_POSITION: &[u8] = b"\x1b[6n";
    if chunk
        .windows(DSR_CURSOR_POSITION.len())
        .any(|w| w == DSR_CURSOR_POSITION)
    {
        let _ = writer.lock().unwrap().write_all(b"\x1b[1;1R");
    }
}
