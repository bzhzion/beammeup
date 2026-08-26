//! Discovery of the interpreters available on THE machine the binary is running on: never a
//! hardcoded path assumed to be identical from one machine to another (observed in testing on
//! Windows, where `bash.exe` alone resolves to the WSL launcher in System32, not Git Bash; the
//! install location of Git for Windows changes depending on whether it was installed for everyone,
//! for the current user, or elsewhere). `beammeup shells` exposes exactly what this function
//! found: the answer to "how do we know what exists on this machine" is "we detect it, we don't
//! configure it by hand".
//!
//! Discovery is specific to each platform (registry and `wsl.exe` on Windows, `$SHELL` and
//! `/etc/shells` on Unix), but the contract exposed to the rest of the program is not:
//! `ShellDescriptor`, `discover()`, and `resolve()` are identical everywhere.

use serde::{Deserialize, Serialize};
// Only used by `push_if_exists`, specific to Windows discovery: each platform module imports
// what it needs.
#[cfg(windows)]
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellDescriptor {
    /// Stable identifier passed to `beammeup open --shell <id>`.
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

/// Adds an interpreter whose path is known in advance, if it actually exists. Only used by
/// Windows discovery, where the locations of `powershell.exe`/`cmd.exe` are derived from
/// `%SystemRoot%`; on the Unix side we start from `$PATH` and `/etc/shells`, plus an executable
/// bit check that this function does not perform.
#[cfg(windows)]
fn push_if_exists(
    out: &mut Vec<ShellDescriptor>,
    id: &str,
    label: &str,
    program: String,
    args: Vec<String>,
) {
    if Path::new(&program).exists() {
        out.push(ShellDescriptor {
            id: id.to_string(),
            label: label.to_string(),
            program,
            args,
        });
    }
}

/// Lists the interpreters actually present on this machine. The first one in the list is the one
/// the window opens by default on startup: the order is therefore not cosmetic.
pub fn discover() -> Vec<ShellDescriptor> {
    #[cfg(windows)]
    {
        windows_shells::discover()
    }
    #[cfg(unix)]
    {
        unix_shells::discover()
    }
}

pub fn resolve(id: &str) -> Result<ShellDescriptor, String> {
    discover()
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| {
            format!(
                "unknown or unavailable shell on this machine: {id} (see `beammeup shells` \
                 for the list of what's actually detected here)"
            )
        })
}

/* -------------------------------------------------------------------------------------------
   Windows
   ------------------------------------------------------------------------------------------- */

#[cfg(windows)]
mod windows_shells {
    use super::{push_if_exists, ShellDescriptor};
    use std::path::Path;
    use std::process::Command;

    fn system_root() -> String {
        std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string())
    }

    /// Git for Windows install path via the registry (reliable regardless of the location
    /// chosen at install time), falling back to the usual locations.
    fn git_for_windows_bash() -> Option<String> {
        for hive_path in [
            r"HKEY_LOCAL_MACHINE\SOFTWARE\GitForWindows",
            r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\GitForWindows",
            r"HKEY_CURRENT_USER\SOFTWARE\GitForWindows",
        ] {
            if let Some(install) = read_registry_string(hive_path, "InstallPath") {
                let candidate = format!(r"{install}\bin\bash.exe");
                if Path::new(&candidate).exists() {
                    return Some(candidate);
                }
            }
        }
        // Fall back to the usual locations if the registry key is absent (older installs,
        // portable, etc.).
        for candidate in [
            format!(
                r"{}\Git\bin\bash.exe",
                std::env::var("ProgramFiles").unwrap_or_default()
            ),
            format!(
                r"{}\Git\bin\bash.exe",
                std::env::var("ProgramFiles(x86)").unwrap_or_default()
            ),
            format!(
                r"{}\Programs\Git\bin\bash.exe",
                std::env::var("LOCALAPPDATA").unwrap_or_default()
            ),
        ] {
            if Path::new(&candidate).exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn read_registry_string(key_path: &str, value_name: &str) -> Option<String> {
        // Read via `reg query` rather than a dedicated registry dependency: sufficient for a
        // handful of one-off reads at startup, not a hot path.
        let output = Command::new("reg")
            .args(["query", key_path, "/v", value_name])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        // Typical line format: "    InstallPath    REG_SZ    C:\Program Files\Git"
        for line in text.lines() {
            if let Some(pos) = line.find("REG_SZ") {
                let value = line[pos + "REG_SZ".len()..].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
        None
    }

    fn powershell7() -> Option<String> {
        for candidate in [
            format!(
                r"{}\PowerShell\7\pwsh.exe",
                std::env::var("ProgramFiles").unwrap_or_default()
            ),
            format!(
                r"{}\PowerShell\7-preview\pwsh.exe",
                std::env::var("ProgramFiles").unwrap_or_default()
            ),
        ] {
            if Path::new(&candidate).exists() {
                return Some(candidate);
            }
        }
        None
    }

    /// Lists installed WSL distributions via `wsl --list --quiet`. Known pitfall: when its
    /// standard output is redirected (not a real terminal), `wsl.exe` encodes it as UTF-16LE
    /// instead of UTF-8: a naive `String::from_utf8_lossy` on the raw bytes produces gibberish
    /// with a null byte between each character.
    fn wsl_distros(wsl_exe: &str) -> Vec<String> {
        let Ok(output) = Command::new(wsl_exe).args(["--list", "--quiet"]).output() else {
            return vec![];
        };
        if !output.status.success() {
            return vec![];
        }
        let utf16: Vec<u16> = output
            .stdout
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&utf16)
            .lines()
            .map(|l| l.trim().trim_end_matches('\0').trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect()
    }

    pub fn discover() -> Vec<ShellDescriptor> {
        let mut out = Vec::new();
        let sysroot = system_root();

        // Windows PowerShell 5.1: present by default on every Windows install since Windows 7/Server 2008 R2.
        push_if_exists(
            &mut out,
            "pwsh5",
            "Windows PowerShell 5.1",
            format!(r"{sysroot}\System32\WindowsPowerShell\v1.0\powershell.exe"),
            vec![],
        );

        // Command Prompt: present by default on every Windows install.
        push_if_exists(
            &mut out,
            "cmd",
            "Command Prompt",
            format!(r"{sysroot}\System32\cmd.exe"),
            // /D disables running the AutoRun commands from the registry. Without it, a freshly
            // launched cmd.exe pulls in whatever is registered under
            // HKCU/HKLM\Software\Microsoft\Command Processor\AutoRun, which on this machine is
            // Clink (which adds readline-style suggestions). Observed in testing: a command sent
            // by the agent was intercepted by a ghost suggestion left over in history
            // ("shutdown /r"), with shutdown.exe actually being invoked (fortunately failing on a
            // syntax error, not executing) before the cause was understood. A session driven by
            // automation must never inherit an interactive suggestion engine designed for a
            // human at the keyboard.
            vec!["/D".to_string()],
        );

        // PowerShell 7+: not installed by default, needs to be detected.
        if let Some(pwsh7) = powershell7() {
            out.push(ShellDescriptor {
                id: "pwsh7".to_string(),
                label: "PowerShell 7".to_string(),
                program: pwsh7,
                args: vec![],
            });
        }

        // Git Bash: not installed by default, path varies depending on the install.
        if let Some(bash) = git_for_windows_bash() {
            out.push(ShellDescriptor {
                id: "gitbash".to_string(),
                label: "Git Bash".to_string(),
                program: bash,
                args: vec!["--login".to_string(), "-i".to_string()],
            });
        }

        // WSL: one entry per installed distribution (never assumes any specific distro exists).
        let wsl_exe = format!(r"{sysroot}\System32\wsl.exe");
        if Path::new(&wsl_exe).exists() {
            for distro in wsl_distros(&wsl_exe) {
                out.push(ShellDescriptor {
                    id: format!("wsl:{distro}"),
                    label: format!("WSL: {distro}"),
                    program: wsl_exe.clone(),
                    args: vec!["-d".to_string(), distro],
                });
            }
        }

        out
    }
}

/* -------------------------------------------------------------------------------------------
   Unix (Linux, macOS)
   ------------------------------------------------------------------------------------------- */

#[cfg(unix)]
mod unix_shells {
    use super::ShellDescriptor;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// Minimal equivalent of `which`: walks `$PATH` and only keeps files that are **actually
    /// executable**. Testing mere existence isn't enough on Unix, where a path can exist without
    /// the executable bit set.
    fn which(name: &str) -> Option<PathBuf> {
        let path_var = std::env::var_os("PATH")?;
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(name))
            .find(|candidate| is_executable(candidate))
    }

    fn is_executable(path: &Path) -> bool {
        path.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    /// Shells declared valid for logging in on this machine. This is the Unix equivalent of
    /// reading the registry on the Windows side: a source of information provided by the system,
    /// not a list we make up ourselves.
    fn etc_shells() -> Vec<PathBuf> {
        let Ok(contents) = std::fs::read_to_string("/etc/shells") else {
            return vec![];
        };
        contents
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(PathBuf::from)
            .collect()
    }

    /// Human-readable name for a given interpreter, derived from the binary's name.
    fn pretty_label(name: &str) -> String {
        match name {
            "bash" => "Bash".to_string(),
            "zsh" => "Zsh".to_string(),
            "fish" => "Fish".to_string(),
            "sh" => "sh (POSIX)".to_string(),
            "dash" => "Dash".to_string(),
            "ksh" => "Ksh".to_string(),
            "tcsh" => "Tcsh".to_string(),
            "csh" => "Csh".to_string(),
            "nu" => "Nushell".to_string(),
            "pwsh" => "PowerShell 7".to_string(),
            "elvish" => "Elvish".to_string(),
            "xonsh" => "Xonsh".to_string(),
            other => other.to_string(),
        }
    }

    /// Candidates looked up in `$PATH` in addition to `$SHELL` and `/etc/shells`, to cover
    /// interpreters that are installed but not declared as login shells (a common case for
    /// `fish`, `nu`, or `pwsh`).
    const KNOWN_SHELLS: &[&str] = &[
        "bash", "zsh", "fish", "sh", "dash", "ksh", "tcsh", "csh", "nu", "pwsh", "elvish", "xonsh",
    ];

    pub fn discover() -> Vec<ShellDescriptor> {
        let mut out: Vec<ShellDescriptor> = Vec::new();
        let mut seen: Vec<PathBuf> = Vec::new();

        let push = |out: &mut Vec<ShellDescriptor>,
                    seen: &mut Vec<PathBuf>,
                    path: PathBuf,
                    is_login_shell: bool| {
            if !is_executable(&path) {
                return;
            }
            // Deduplicate on the resolved path: `/bin/bash` and `/usr/bin/bash` are the same
            // interpreter on any modern distribution (`/bin` is a symlink to `/usr/bin`).
            let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if seen.contains(&canonical) {
                return;
            }
            seen.push(canonical);

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "shell".to_string());
            let label = if is_login_shell {
                format!("{} (default shell)", pretty_label(&name))
            } else {
                pretty_label(&name)
            };
            out.push(ShellDescriptor {
                id: name,
                label,
                program: path.to_string_lossy().to_string(),
                // No arguments: launched in a real PTY, a shell already considers itself
                // interactive. `--login` is deliberately avoided, since it changes which
                // configuration files get loaded (`~/.profile` instead of `~/.bashrc`) and
                // doesn't match what a classic terminal emulator does.
                args: vec![],
            });
        };

        // 1. The user's login shell first: that's the one the window opens by default, so it's
        //    the one they expect.
        if let Some(shell) = std::env::var_os("SHELL") {
            push(&mut out, &mut seen, PathBuf::from(shell), true);
        }

        // 2. What the system itself declares as valid login shells.
        for path in etc_shells() {
            push(&mut out, &mut seen, path, false);
        }

        // 3. Known interpreters present in PATH but absent from /etc/shells.
        for name in KNOWN_SHELLS {
            if let Some(path) = which(name) {
                push(&mut out, &mut seen, path, false);
            }
        }

        // Safety net: on a very minimal system (container), neither `$SHELL` nor `/etc/shells`
        // are necessarily set, but `/bin/sh` always exists.
        if out.is_empty() {
            push(&mut out, &mut seen, PathBuf::from("/bin/sh"), false);
        }

        out
    }
}
