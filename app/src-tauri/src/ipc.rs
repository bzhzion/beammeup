//! Local control transport between the CLI connector and the already-open GUI window.
//!
//! Two implementations, both tested on their platform: named pipe on Windows,
//! Unix domain socket on Linux (validated on 2026-08-25 under WSLg, Debian 13). macOS shares the
//! `cfg(unix)` code path but hasn't been run yet.
//!
//! Both aim for the same security property: **the control channel grants full shell
//! access, so it must never be reachable by another account on the machine.** Windows
//! achieves this with a DACL restricted to the current SID, Unix with the location
//! (`$XDG_RUNTIME_DIR`, already 0700) combined with explicit 0600 permissions on the socket.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::protocol::{Request, Response};
use crate::session::SessionManager;

/// Is an instance currently responding on the control channel?
///
/// Synchronous, side-effect-free probe used during elevation on Unix: it's the only
/// reliable signal that an elevated window has actually started, since the authentication
/// process itself stays alive for as long as the window it launched.
#[cfg(unix)]
pub fn control_channel_is_live() -> bool {
    std::os::unix::net::UnixStream::connect(unix_impl::socket_path()).is_ok()
}

const CONNECT_RETRY_TOTAL: Duration = Duration::from_secs(15);
const CONNECT_RETRY_STEP: Duration = Duration::from_millis(200);

/// Window actions triggered by commands received on the pipe, grouped together so the
/// parameter list of `serve`/`handle_connection` doesn't keep growing with each
/// new action (show/focus, quit, fullscreen, and whatever comes next).
#[derive(Clone)]
pub struct WindowControls {
    /// Called for EVERY received command (except `Quit`), before it's even executed, so the
    /// window consistently comes to the foreground (the "never headless mode" rule).
    pub show_and_focus: Arc<dyn Fn() + Send + Sync>,
    /// Called once the response to `Request::Quit` has been sent: the only clean way to close
    /// BeamMeUp from the outside, since an elevated process can't be killed by a non-elevated
    /// `taskkill`/`Stop-Process` (`Access is denied`, observed in testing).
    pub quit: Arc<dyn Fn() + Send + Sync>,
    pub set_fullscreen: Arc<dyn Fn(bool) + Send + Sync>,
    /// Live handle to the optional remote web server (see `remote_web.rs`), started/stopped on
    /// the already-running instance without a restart, the same way `set_fullscreen` toggles
    /// fullscreen live.
    pub remote_web: Arc<crate::remote_web::RemoteWebHandle>,
}

/// Launches the control server and never returns while the app is running.
pub async fn serve(manager: Arc<SessionManager>, controls: WindowControls) {
    #[cfg(windows)]
    windows_impl::serve(manager, controls).await;

    #[cfg(unix)]
    unix_impl::serve(manager, controls).await;
}

/// Sends a request to the already-open instance; if no instance responds, relaunches
/// `beammeup.exe` with no arguments (visible window guaranteed, never headless) and retries until
/// `CONNECT_RETRY_TOTAL`.
pub async fn send_request(req: &Request) -> Result<Response, String> {
    match try_connect_and_send(req).await {
        Ok(resp) => return Ok(resp),
        Err(e) if is_not_running(&e) => {}
        Err(e) => return Err(format!("connection error: {e}")),
    }

    spawn_gui_detached()?;

    let deadline = std::time::Instant::now() + CONNECT_RETRY_TOTAL;
    loop {
        tokio::time::sleep(CONNECT_RETRY_STEP).await;
        match try_connect_and_send(req).await {
            Ok(resp) => return Ok(resp),
            Err(e) if is_not_running(&e) && std::time::Instant::now() < deadline => continue,
            Err(e) => {
                return Err(format!(
                    "unable to reach BeamMeUp after relaunch ({e})"
                ))
            }
        }
    }
}

fn spawn_gui_detached() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    #[allow(unused_mut)]
    let mut command = std::process::Command::new(exe);

    // On Unix, a child stays in its parent's process group: when the shell that
    // launched the CLI connector exits, the window receives the `SIGHUP` meant for the group and
    // dies with it. `setsid()` puts it in its own session, so it survives the caller, which
    // is the expected behavior (the window must stay open after the command). We also
    // close the inherited standard streams, otherwise the window would keep the calling
    // shell's pipes open, and it would then wait for them to close.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    command
        .spawn()
        .map_err(|e| format!("failed to relaunch the window: {e}"))?;
    Ok(())
}

async fn try_connect_and_send(req: &Request) -> Result<Response, io::Error> {
    #[cfg(windows)]
    let stream = windows_impl::connect().await?;
    #[cfg(unix)]
    let stream = unix_impl::connect().await?;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    let mut line = serde_json::to_string(req).expect("serializable Request");
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;

    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).await?;
    serde_json::from_str(resp_line.trim()).map_err(|e| io::Error::other(format!("invalid response: {e}")))
}

/// Must be called first, before building anything on the window side. Returns `true`
/// if an existing instance responded (and has therefore already been brought to the foreground by
/// `show_and_focus`, as for any other request): in that case the current process must
/// stop immediately instead of building its own independent window.
///
/// **Why this guard.** Before it was added (2026-08-25), `ipc::serve` silently failed
/// to create the pipe if an instance was already running, and the function returned without ever
/// preventing Tauri from building a window: two `beammeup.exe` launched back to back would
/// produce two independent windows, the second one invisible to the CLI (which only talks to
/// the pipe's owner) and uncoordinated with the first, observed in testing, a source of repeated
/// confusion about session state.
pub fn another_instance_is_running_and_focused() -> bool {
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(_) => return false,
    };
    rt.block_on(async {
        // Several attempts with a more generous delay: a single 500ms try produced
        // false negatives (the existing instance still starting up, or just slow to
        // respond under load), observed in testing on 2026-08-25, exactly the scenario that
        // lets a second independent window get built.
        for tentative in 0..5 {
            if tentative > 0 {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            if tokio::time::timeout(Duration::from_secs(2), try_connect_and_send(&Request::Status))
                .await
                .map(|r| r.is_ok())
                .unwrap_or(false)
            {
                return true;
            }
        }
        false
    })
}

#[cfg(windows)]
fn is_not_running(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(2) | Some(231) // ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY
    )
}

#[cfg(unix)]
fn is_not_running(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
}

async fn handle_connection<S>(stream: S, manager: Arc<SessionManager>, controls: WindowControls)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
    }
    let Ok(req) = serde_json::from_str::<Request>(line.trim()) else {
        return;
    };

    let is_quit = matches!(req, Request::Quit);
    if !is_quit {
        (controls.show_and_focus)();
    }

    // `Exec` can block for several seconds (waiting for a command to finish): `spawn_blocking`
    // so it doesn't freeze the tokio runtime for that whole time, unlike the other requests
    // which are fast enough to stay synchronous inside `dispatch`.
    let resp = if let Request::Exec {
        id,
        command,
        timeout_ms,
    } = req
    {
        let manager = manager.clone();
        tokio::task::spawn_blocking(move || {
            match manager.exec(&id, &command, timeout_ms.unwrap_or(15_000)) {
                Ok((output, exit_code, timed_out)) => Response::Exec {
                    output,
                    exit_code,
                    timed_out,
                },
                Err(message) => Response::Error { message },
            }
        })
        .await
        .unwrap_or_else(|e| Response::Error {
            message: format!("exec failed: {e}"),
        })
    } else {
        dispatch(&manager, req, &controls)
    };

    let mut out = serde_json::to_string(&resp).unwrap_or_else(|_| {
        r#"{"status":"error","message":"response serialization failed"}"#.to_string()
    });
    out.push('\n');
    let _ = writer.write_all(out.as_bytes()).await;
    let _ = writer.flush().await;

    // The response must have gone out over the wire BEFORE quitting, otherwise the CLI connector
    // sees the connection close with no response and reports an error instead of a clean confirmation.
    if is_quit {
        (controls.quit)();
    }
}

fn dispatch(manager: &Arc<SessionManager>, req: Request, controls: &WindowControls) -> Response {
    match req {
        Request::Status => Response::Status {
            window_visible: true,
            session_count: manager.count(),
        },
        Request::List => Response::List {
            sessions: manager.list(),
        },
        Request::ListShells => Response::Shells {
            shells: crate::shells::discover(),
        },
        Request::Open {
            shell,
            ssh_args,
            scp_args,
            cols,
            rows,
            label,
        } => match manager.open(shell, ssh_args, scp_args, cols, rows, label) {
            Ok(id) => Response::Opened { id },
            Err(message) => Response::Error { message },
        },
        Request::Send { id, text } => match manager.send_input(&id, &text) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { message },
        },
        // Intercepted earlier in `handle_connection` (via `spawn_blocking`, potentially
        // blocking for several seconds): this branch is in practice never reached, kept
        // only for the exhaustiveness of the `match`.
        Request::Exec {
            id,
            command,
            timeout_ms,
        } => match manager.exec(&id, &command, timeout_ms.unwrap_or(15_000)) {
            Ok((output, exit_code, timed_out)) => Response::Exec {
                output,
                exit_code,
                timed_out,
            },
            Err(message) => Response::Error { message },
        },
        Request::Key { id, key } => match manager.send_key(&id, &key) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { message },
        },
        Request::Read {
            id,
            since,
            last,
            plain,
        } => match manager.read(&id, since.unwrap_or(0), last, plain) {
            Ok((data, cursor, alive)) => Response::Output {
                data,
                cursor,
                alive,
            },
            Err(message) => Response::Error { message },
        },
        Request::Resize { id, cols, rows } => match manager.resize(&id, cols, rows) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { message },
        },
        Request::SelectTab { id } => match manager.select(&id) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { message },
        },
        Request::Close { id } => match manager.close(&id) {
            Ok(()) => Response::Ok,
            Err(message) => Response::Error { message },
        },
        Request::CloseAll => Response::Closed {
            count: manager.close_all(),
        },
        Request::DuplicateTab { id } => match manager.duplicate(&id) {
            Ok(id) => Response::Opened { id },
            Err(message) => Response::Error { message },
        },
        Request::ReopenLastClosed => match manager.reopen_last_closed() {
            Ok(id) => Response::Opened { id },
            Err(message) => Response::Error { message },
        },
        Request::Quit => {
            manager.close_all();
            Response::Ok
        }
        Request::SetFullscreen { enabled } => {
            (controls.set_fullscreen)(enabled);
            Response::Ok
        }
        Request::WebOn {
            bind,
            token,
            no_token,
        } => {
            let mut config = crate::remote_web::load_config();
            let resolved_bind = bind.unwrap_or_else(|| config.bind.clone());
            let resolved_token = if no_token {
                None
            } else if token.is_some() {
                token
            } else {
                Some(crate::remote_web::generate_token())
            };
            match controls
                .remote_web
                .start(manager.clone(), resolved_bind, resolved_token.clone())
            {
                Ok(actual_bind) => {
                    config.bind = actual_bind.clone();
                    config.token = resolved_token.clone();
                    // The user just explicitly asked for this to run: persist it so it comes
                    // back automatically on the next launch too (see `lib.rs`'s `setup()`),
                    // without having to run `beammeup web on` again after every restart.
                    config.autostart = true;
                    if let Err(e) = crate::remote_web::save_config(&config) {
                        eprintln!("beammeup: failed to save remote.json ({e})");
                    }
                    Response::WebStarted {
                        bind: actual_bind,
                        token: resolved_token,
                    }
                }
                Err(message) => Response::Error { message },
            }
        }
        Request::WebOff => {
            controls.remote_web.stop();
            // Symmetric with `WebOn`: an explicit `web off` must also survive a restart, or the
            // server would silently come back the next time the window launches.
            let mut config = crate::remote_web::load_config();
            config.autostart = false;
            if let Err(e) = crate::remote_web::save_config(&config) {
                eprintln!("beammeup: failed to save remote.json ({e})");
            }
            Response::Ok
        }
        Request::WebStatus => {
            let (running, bind, token_set) = controls.remote_web.status();
            Response::WebStatus {
                running,
                bind,
                token_set,
            }
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    fn pipe_path() -> String {
        format!(r"\\.\pipe\{}", crate::protocol::control_channel_name())
    }

    pub async fn connect() -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        ClientOptions::new().open(pipe_path())
    }

    /// SID (string form, e.g. `S-1-5-21-...`) of the Windows account the current process is
    /// running under. Used to scope access to the pipe to THIS specific account, rather than
    /// "Everyone" (`WD`): on a machine shared by several Windows accounts, `WD` would give
    /// any other logged-in user (even non-admin) full access to the control pipe of an
    /// elevated (admin) BeamMeUp instance running under a different account: opening
    /// sessions, sending text, closing them, a trivial local privilege escalation between
    /// accounts. Found during the 2026-08-24 security audit, fixed here.
    fn current_user_sid_string() -> io::Result<String> {
        unsafe {
            let mut token = windows::Win32::Foundation::HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
                .map_err(|e| io::Error::other(format!("OpenProcessToken: {e}")))?;

            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
            let mut buf = vec![0u8; needed as usize];
            let ok = GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr() as *mut _),
                needed,
                &mut needed,
            );
            let _ = CloseHandle(token);
            ok.map_err(|e| io::Error::other(format!("GetTokenInformation(TokenUser): {e}")))?;

            let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut sid_ptr = windows::core::PWSTR::null();
            ConvertSidToStringSidW(token_user.User.Sid, &mut sid_ptr)
                .map_err(|e| io::Error::other(format!("ConvertSidToStringSidW: {e}")))?;
            let sid_string = sid_ptr.to_string().map_err(|e| io::Error::other(e.to_string()));
            LocalFree(Some(HLOCAL(sid_ptr.0 as *mut _)));
            sid_string
        }
    }

    /// Creates a pipe instance restricted to the current Windows account, with an explicit
    /// Low integrity level (`S:(ML;;NW;;;LW)`). The Low integrity level is needed because
    /// BeamMeUp self-elevates (High IL): without it, the CLI connector (never elevated, Medium IL)
    /// gets denied access to the pipe by Windows' Mandatory Integrity Control (observed
    /// in testing: ERROR_ACCESS_DENIED), even though the underlying DACL allows the right account. The
    /// DACL itself only allows the current account's SID (never `WD`/Everyone): the integrity
    /// control protects against less-elevated processes of the SAME account, not against a
    /// different Windows account.
    fn create_pipe_instance(first_instance: bool) -> io::Result<NamedPipeServer> {
        let path = pipe_path();
        let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let sid = current_user_sid_string()?;
        let sddl: Vec<u16> = format!("D:(A;;GA;;;{sid})S:(ML;;NW;;;LW)\0")
            .encode_utf16()
            .collect();

        let mut psd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut psd,
                None,
            )
            .map_err(|e| io::Error::other(format!("invalid SDDL: {e}")))?;
        }

        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: psd.0,
            bInheritHandle: false.into(),
        };

        let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        if first_instance {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }

        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(wide_path.as_ptr()),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                Some(&sa),
            )
        };

        unsafe {
            LocalFree(Some(HLOCAL(psd.0)));
        }

        if handle.is_invalid() {
            return Err(io::Error::last_os_error());
        }

        unsafe { Ok(NamedPipeServer::from_raw_handle(handle.0 as _)?) }
    }

    pub async fn serve(manager: Arc<SessionManager>, controls: WindowControls) {
        let mut server = match create_pipe_instance(true) {
            Ok(s) => s,
            Err(e) => {
                // `another_instance_is_running_and_focused()` (called before the window is even
                // built) did not detect an existing instance, but creating the pipe still fails:
                // proof that another instance really does exist (just too slow to
                // respond at that moment for the earlier check to see it). Continuing
                // would leave an orphan window running, invisible to the CLI and uncoordinated
                // with the real instance, observed in testing on 2026-08-25. Better to stop
                // the whole process (window included) than to duplicate the window.
                eprintln!("beammeup: failed to create the control pipe ({e}), already running?");
                std::process::exit(1);
            }
        };

        loop {
            if server.connect().await.is_ok() {
                let connected = server;
                // Prepare the next instance before handling the current connection, so
                // a concurrent second call doesn't needlessly hit ERROR_PIPE_BUSY.
                server = match create_pipe_instance(false) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("beammeup: failed to recreate the pipe ({e})");
                        return;
                    }
                };

                // The pipe's DACL already restricts access to the current account's SID (see
                // above), but any other program running under that same account could
                // still connect to it and drive the sessions like a real BeamMeUp connector.
                // Additionally checking that the calling process is literally the same executable
                // closes that gap (found while studying the competition, Unterm distinguishes
                // each calling agent, which BeamMeUp didn't do at all until now) without needing
                // a per-agent authorization banner: unlike Unterm, designed to be
                // driven by various third-party tools, BeamMeUp is never meant to be
                // spoken to by anything other than itself (the agent always calls the same `beammeup.exe`).
                if !client_is_beammeup(&connected) {
                    eprintln!("beammeup: connection refused (calling process not recognized)");
                    continue;
                }

                let manager = manager.clone();
                let controls = controls.clone();
                tokio::spawn(async move {
                    handle_connection(connected, manager, controls).await;
                });
            } else {
                break;
            }
        }
    }

    /// `true` if the process that just connected to the pipe is indeed an instance of
    /// `beammeup.exe` (comparison of the resolved executable path, not just the name). Refuses out of
    /// caution if the check itself fails (process already terminated, access denied...).
    fn client_is_beammeup(pipe: &NamedPipeServer) -> bool {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;

        let handle = HANDLE(pipe.as_raw_handle());
        let mut pid: u32 = 0;
        let ok = unsafe { GetNamedPipeClientProcessId(handle, &mut pid) };
        if ok.is_err() {
            return false;
        }
        match (client_image_path(pid), std::env::current_exe()) {
            (Some(client_path), Ok(self_path)) => client_path == self_path,
            _ => false,
        }
    }

    fn client_image_path(pid: u32) -> Option<std::path::PathBuf> {
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(handle);
            if ok.is_err() {
                return None;
            }
            Some(std::path::PathBuf::from(String::from_utf16_lossy(
                &buf[..len as usize],
            )))
        }
    }
}

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::{UnixListener, UnixStream};

    /// UID of the **real** user, i.e. whoever launched BeamMeUp, even when the
    /// window has re-executed itself as root via `pkexec`/`sudo`.
    ///
    /// This is the linchpin of how elevation works: the elevated window and the CLI
    /// connector (never elevated) must end up on **the same** socket. If the window based itself
    /// on its own UID once running as root, it would listen in `/run/user/0` while the CLI
    /// would look in `/run/user/1000`, and nothing would talk to each other anymore. It's the exact Linux
    /// equivalent of the Mandatory Integrity Control pitfall encountered on the Windows side.
    pub fn real_uid() -> u32 {
        for var in ["PKEXEC_UID", "SUDO_UID"] {
            if let Ok(value) = std::env::var(var) {
                if let Ok(uid) = value.parse::<u32>() {
                    return uid;
                }
            }
        }
        unsafe { libc::getuid() }
    }

    /// Folder in which to place the control socket, always that of the real user.
    ///
    /// `/run/user/<uid>` is the intended location for this kind of socket (created by systemd,
    /// already 0700, cleaned up on logout). We rebuild it from the UID rather than reading
    /// `$XDG_RUNTIME_DIR`, because that variable points to root's when running elevated.
    /// Falls back to `$XDG_RUNTIME_DIR` (systems without `/run/user`, macOS), then to a
    /// private subfolder of `/tmp`.
    ///
    /// The last fallback is **not** `/tmp` directly: `/tmp` is world-writable, placing
    /// the socket there directly would make it reachable by any account on the machine.
    fn runtime_dir() -> std::path::PathBuf {
        let uid = real_uid();

        let by_uid = std::path::PathBuf::from(format!("/run/user/{uid}"));
        if by_uid.is_dir() {
            return by_uid;
        }

        if let Some(from_env) = std::env::var_os("XDG_RUNTIME_DIR") {
            let path = std::path::PathBuf::from(from_env);
            if path.is_dir() {
                return path;
            }
        }

        let fallback = std::env::temp_dir().join(format!("beammeup-{uid}"));
        let _ = std::fs::create_dir_all(&fallback);
        let _ = std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o700));
        fallback
    }

    /// File name derived from the UID rather than the username: under `pkexec`, `$USER` is
    /// `root` on the window side and the original user on the CLI side, which would give two
    /// different names for the same channel.
    pub(super) fn socket_path() -> std::path::PathBuf {
        runtime_dir().join(format!("beammeup-{}.sock", real_uid()))
    }

    pub async fn connect() -> io::Result<UnixStream> {
        UnixStream::connect(socket_path()).await
    }

    pub async fn serve(manager: Arc<SessionManager>, controls: WindowControls) {
        let path = socket_path();

        // An already-present socket doesn't necessarily mean an instance is running: it could
        // be orphaned (a previous crash). We decide by trying to connect to it, instead of
        // blindly removing it: removing the socket of a live instance would leave it running with
        // a listener nobody can reach anymore, and we'd end up with two windows.
        // This is the equivalent of `FILE_FLAG_FIRST_PIPE_INSTANCE` used on the Windows side.
        if path.exists() {
            match UnixStream::connect(&path).await {
                Ok(_) => {
                    // Same reasoning as the bind-failure branch below: the Tauri window has
                    // already been built by the time `serve()` runs, so a bare `return` here
                    // would leave that window alive with no working control channel, a second,
                    // CLI-unreachable instance driving its own PTYs. Exit the whole process
                    // instead (mirrors the Windows side's `another_instance_is_running_and_focused`
                    // check, which is meant to catch this earlier, but this is the fallback of
                    // last resort if that race is lost).
                    eprintln!(
                        "beammeup: an instance is already responding on {}, this one won't start a \
                         control server",
                        path.display()
                    );
                    std::process::exit(1);
                }
                Err(_) => {
                    // Nobody listening: stale socket, we can replace it.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }

        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                // Same reasoning as on the Windows side (see its comment): don't leave
                // an orphan window running if the socket truly cannot be created.
                eprintln!("beammeup: failed to bind the control socket ({e})");
                std::process::exit(1);
            }
        };

        // Explicit permissions rather than relying on the process's `umask`, which depends on
        // the environment the window was launched from. Defense in depth: even inside
        // `/run/user/<uid>` (already private), the socket itself is only reachable by its
        // owner.
        if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!("beammeup: unable to restrict the socket's permissions ({e})");
            let _ = std::fs::remove_file(&path);
            return;
        }

        // When the window is running elevated, the socket is owned by root: at 0600 it would then
        // be unreachable by the CLI connector, which is never elevated. We hand it back to the
        // real user so that only they (and root) can connect to it. Connecting to a Unix socket
        // requires write permission on it, so this chown is what makes control possible
        // without opening access to other accounts on the machine.
        let uid = real_uid();
        if uid != unsafe { libc::geteuid() } {
            let c_path = match std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("beammeup: invalid socket path ({e})");
                    let _ = std::fs::remove_file(&path);
                    return;
                }
            };
            // gid left unchanged (-1): only user ownership matters for 0600 access.
            if unsafe { libc::chown(c_path.as_ptr(), uid, u32::MAX) } != 0 {
                eprintln!(
                    "beammeup: unable to hand the socket back to user {uid} ({}), the \
                     CLI connector won't be able to connect to it",
                    std::io::Error::last_os_error()
                );
                let _ = std::fs::remove_file(&path);
                return;
            }
        }

        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(e) => {
                    // `break` and not `continue`: an accept error is in practice permanent
                    // (exhausted descriptors, closed listener), and looping on it would burn a
                    // whole core without ever making progress. Same choice as on the Windows side.
                    eprintln!("beammeup: accept on the control socket failed ({e})");
                    break;
                }
            };

            // Same reasoning as on the Windows side (`client_is_beammeup`): the socket's 0600
            // permissions already rule out other Unix accounts, but not another
            // program running under the same user. `SO_PEERCRED` gives the real PID of
            // the caller, resolved next via `/proc/<pid>/exe`.
            if !client_is_beammeup(&stream) {
                eprintln!("beammeup: connection refused (calling process not recognized)");
                continue;
            }

            let manager = manager.clone();
            let controls = controls.clone();
            tokio::spawn(async move {
                handle_connection(stream, manager, controls).await;
            });
        }

        // Don't leave an orphan socket behind: without this, the next startup has to
        // rely on the detection above to clean it up.
        let _ = std::fs::remove_file(&path);
    }

    /// `true` if the process that just connected to the socket is indeed an instance of
    /// `beammeup` (comparison of the resolved executable path). `SO_PEERCRED` is Linux-specific;
    /// macOS (never run to this day, see the comment at the top of the file) stays
    /// permissive rather than breaking an untested path, not a regression: this check
    /// simply didn't exist at all before.
    #[cfg(target_os = "linux")]
    fn client_is_beammeup(stream: &UnixStream) -> bool {
        use std::os::unix::io::AsRawFd;

        let fd = stream.as_raw_fd();
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let ret = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut libc::ucred as *mut libc::c_void,
                &mut len,
            )
        };
        if ret != 0 {
            return false;
        }
        let client_exe = std::path::PathBuf::from(format!("/proc/{}/exe", cred.pid));
        match (std::fs::read_link(&client_exe), std::env::current_exe()) {
            (Ok(client_path), Ok(self_path)) => client_path == self_path,
            _ => false,
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn client_is_beammeup(_stream: &UnixStream) -> bool {
        true
    }
}
