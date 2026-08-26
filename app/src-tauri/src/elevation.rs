//! Elevation of the GUI window only (never the CLI connector, see app.manifest).
//!
//! **Windows.** The manifest stays `asInvoker` so that `beammeup status/open/send/...` keeps
//! normal stdin/stdout redirection. When the binary starts in GUI mode (no arguments), we
//! check here whether the current process is already elevated; if not, we relaunch ourselves via
//! `ShellExecuteW` with the `runas` verb (triggers the real UAC prompt, only once per
//! Windows session as long as the window is running), then exit this non-elevated process.
//!
//! **Linux.** Same idea via `pkexec` (falling back to `sudo`), with two deliberate differences:
//!
//! 1. **A refusal is not fatal.** On Windows, refusing UAC closes the application: a
//!    non-elevated PowerShell session wouldn't offer what's expected of the tool there. On Linux,
//!    by contrast, a regular user terminal is perfectly useful, and per-command `sudo` is the
//!    norm there. We warn on stderr and continue without root privileges, rather than leaving
//!    the user without any window at all.
//! 2. **The graphical environment must be forwarded explicitly.** `pkexec` sanitizes
//!    the environment for security reasons, including `DISPLAY`/`WAYLAND_DISPLAY`. Without
//!    passing them back through, the root process has no display server to reach and no window
//!    opens.
//!
//! Worth knowing before relying on this: running a web engine (WebKitGTK) as root significantly
//! widens the attack surface. The alternative, if this trade-off becomes a problem, is to keep the
//! window running as the regular user and elevate on a per-session basis instead (the shell launched
//! under `pkexec` inside the PTY, not the window).

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Marker file written before each relaunch attempt, so a second one is never tried. Guard
/// added on 2026-08-25 after an infinite relaunch loop was observed in testing (dozens of
/// stacked `beammeup.exe` processes, several of them unreachable by a non-elevated
/// `taskkill`): `is_elevated()` can be wrong on a machine where UAC is disabled, since the
/// process token there never carries the "elevated" flag in the `TOKEN_ELEVATION` sense, even
/// when running as admin, so each relaunch in turn perceived itself as non-elevated. Rather than
/// fixing the detection itself (not reproduced in the dev environment), this guard makes a
/// detection failure harmless: a single attempt, then we carry on non-elevated instead of looping.
///
/// **A file, not an environment variable.** `ShellExecuteW` with the `runas` verb goes
/// through the Windows elevation service (AppInfo/consent.exe), which builds a fresh environment
/// block for the elevated process rather than inheriting the caller's; a variable set here via
/// `std::env::set_var` is therefore not guaranteed to survive the UAC boundary. A file on
/// disk, on the other hand, is visible to every process regardless of its token.
#[cfg(windows)]
fn marqueur_relance_path() -> std::path::PathBuf {
    std::env::temp_dir().join("beammeup_relance_elevation.marker")
}

/// `true` if a relaunch for elevation has already been attempted recently (under 60s; beyond
/// that, the marker is treated as stale rather than blocking a future legitimate attempt
/// indefinitely, for example after a Windows session restart). Must be checked before calling
/// `relaunch_elevated_and_exit`, so a second attempt is never made.
#[cfg(windows)]
pub fn relance_deja_tentee() -> bool {
    match std::fs::metadata(marqueur_relance_path()).and_then(|m| m.modified()) {
        Ok(modifie) => modifie
            .elapsed()
            .map(|age| age < std::time::Duration::from_secs(60))
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Never returns: either the elevated relaunch was triggered and this process stops, or
/// the user refused the UAC prompt and we stop anyway (no non-elevated window, so we never end
/// up with a PowerShell tab that doesn't inherit the expected admin rights).
#[cfg(windows)]
pub fn relaunch_elevated_and_exit() -> ! {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let _ = std::fs::write(marqueur_relance_path(), b"1");

    let exe = std::env::current_exe().expect("current_exe");
    let exe_wide: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb_wide: Vec<u16> = "runas\0".encode_utf16().collect();

    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_wide.as_ptr()),
            PCWSTR(exe_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
    std::process::exit(0);
}

/* -------------------------------------------------------------------------------------------
   Unix
   ------------------------------------------------------------------------------------------- */

#[cfg(unix)]
pub fn is_elevated() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Variables without which a root process cannot find the user's display server.
/// `pkexec` starts from a minimal environment for security reasons: whatever isn't forwarded here
/// is lost, and the window doesn't open.
#[cfg(unix)]
const GRAPHICAL_ENV_VARS: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Timeout beyond which authentication is considered unlikely to succeed.
///
/// Long enough to let someone read the prompt and type their password, short enough not to
/// leave the application stuck without a window. This isn't theoretical: observed in testing under
/// WSLg, `pkexec` **waits indefinitely** when no authentication agent is available
/// (no graphical desktop, no controlling terminal). Without this guard, BeamMeUp would
/// never start on a machine in that situation, which is far worse than running without
/// root privileges.
#[cfg(unix)]
const ELEVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Attempts to relaunch the window with root privileges.
///
/// Unlike the Windows version, does **not** terminate on failure or refusal: returns
/// `false` so the caller continues without elevation, with a normal user-level window.
#[cfg(unix)]
pub fn try_relaunch_elevated() -> bool {
    use std::process::Command;

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    // `pkexec` first: it's the one that shows a real graphical authentication prompt.
    // `sudo` as a fallback only helps if a token is already cached or if sudo is configured
    // passwordless: it has no graphical interface and cannot prompt for a password here,
    // since the window isn't launched from any terminal.
    for helper in ["pkexec", "sudo"] {
        if which_in_path(helper).is_none() {
            continue;
        }

        // Don't launch `pkexec` if it has no way to request authentication: it would then
        // wait forever, and the user would see the window take a full minute to
        // appear (the length of the guard) instead of starting immediately. Observed under
        // WSLg, where polkitd is running but no desktop provides an agent.
        if helper == "pkexec" && !polkit_agent_available() {
            eprintln!(
                "beammeup: no polkit authentication agent detected, elevation skipped (the \
                 window starts with the current user's privileges)"
            );
            continue;
        }

        let mut command = Command::new(helper);
        if helper == "sudo" {
            // Never block on a password prompt that no terminal will display.
            command.arg("-n");
        }
        // Forward the real UID: the elevated window needs it to place its control socket
        // in the right spot (the user's, not root's), otherwise the CLI connector can no longer
        // find it. `pkexec` sets `PKEXEC_UID` itself, but `sudo -n` doesn't do so reliably
        // when going through `env`.
        command.arg("env");
        command.arg(format!("PKEXEC_UID={}", unsafe { libc::getuid() }));
        for var in GRAPHICAL_ENV_VARS {
            if let Ok(value) = std::env::var(var) {
                command.arg(format!("{var}={value}"));
            }
        }
        command.arg(&exe);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("beammeup: failed to launch {helper} ({e})");
                continue;
            }
        };

        // We can't simply wait for the process to finish: on success, it lives as
        // long as the elevated window, i.e. potentially for hours. The observable signal
        // of a successful elevation is the control socket appearing, which only the
        // window creates. So we watch both: the socket (success) and the process exiting
        // (failure), with a maximum timeout so we never stay stuck.
        let deadline = std::time::Instant::now() + ELEVATION_TIMEOUT;
        loop {
            if crate::ipc::control_channel_is_live() {
                // The elevated window is responding: this non-elevated process has no reason to exist anymore.
                std::process::exit(0);
            }
            match child.try_wait() {
                Ok(Some(_)) => {
                    eprintln!(
                        "beammeup: elevation via {helper} refused or impossible, the window \
                         starts with the current user's privileges (use `sudo` inside \
                         a session for commands that need it)"
                    );
                    return false;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("beammeup: unable to track {helper} ({e})");
                    return false;
                }
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!(
                    "beammeup: authentication via {helper} did not complete within the \
                     allotted time (no authentication agent available?), the window starts \
                     with the current user's privileges"
                );
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    eprintln!(
        "beammeup: neither pkexec nor sudo is usable, the window starts with the \
         current user's privileges"
    );
    false
}

/// Is a polkit authentication agent running?
///
/// `pkexec` delegates the password prompt to an agent provided by the desktop environment;
/// without an agent **and** without a controlling terminal, it waits indefinitely instead of
/// failing. So we detect its presence upfront rather than suffer that hang.
///
/// Detection is done by process name in `/proc`: approximate but good enough, and without
/// a D-Bus dependency. A full desktop environment (GNOME, KDE, XFCE...) always provides one
/// of these processes; a session without a desktop (WSLg, server, container) has none.
#[cfg(unix)]
fn polkit_agent_available() -> bool {
    const AGENTS: &[&str] = &[
        "polkit-gnome-au", // name truncated to 15 characters in /proc/<pid>/comm
        "polkit-kde-auth",
        "polkit-mate-aut",
        "lxpolkit",
        "xfce-polkit",
        "gnome-shell",
        "plasmashell",
        "polkit-agent-he",
    ];

    let Ok(entries) = std::fs::read_dir("/proc") else {
        // No /proc (macOS notably): we can't conclude, so we let it try rather
        // than wrongly disabling elevation. The timeout guard still applies.
        return true;
    };

    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
            let comm = comm.trim();
            if AGENTS.iter().any(|agent| comm == *agent) {
                return true;
            }
        }
    }
    false
}

#[cfg(unix)]
fn which_in_path(name: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| {
            candidate
                .metadata()
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}
