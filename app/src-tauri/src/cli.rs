use clap::{Parser, Subcommand};

use crate::ipc;
use crate::protocol::{Request, Response};

#[derive(Parser)]
#[command(name = "beammeup", about = "Shared human + agent PowerShell/SSH sessions")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Is the window running, and how many sessions are open
    Status,
    /// List the open sessions
    List,
    /// List the interpreters actually detected on this machine (PowerShell 5.1/7, cmd,
    /// Git Bash, WSL distributions). Never a fixed list, each machine can differ.
    Shells,
    /// Closes every session then quits the application (the only clean way to close it
    /// from the outside: an elevated process cannot be killed by a non-elevated Stop-Process)
    Quit,
    /// Toggles the window fullscreen state on or off
    Fullscreen {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Captures a real screenshot of the window (the active tab as seen by the human) via the
    /// WebView2 debug port. Relaunches the window if needed, like any other command.
    Screenshot {
        /// Path of the PNG to write (defaults to a temp file whose path is printed)
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Opens a new tab (always a new session, never silently reused)
    Open {
        /// Id returned by `beammeup shells` (e.g. "pwsh5", "gitbash", "wsl:Debian"). Mutually
        /// exclusive with --ssh.
        #[arg(long)]
        shell: Option<String>,
        /// Arguments passed as-is to the system's ssh client (e.g. "kao@axolotl -i C:\path\key").
        /// Mutually exclusive with --shell/--scp.
        #[arg(long)]
        ssh: Option<String>,
        /// Arguments passed as-is to the system's scp client for a visible file transfer
        /// (e.g. "C:\local\file.txt kao@axolotl:/remote/" to upload, or the reverse to
        /// download). Reuses the same SSH key/agent as --ssh. Mutually exclusive
        /// with --shell/--ssh.
        #[arg(long)]
        scp: Option<String>,
        #[arg(long)]
        cols: Option<u16>,
        #[arg(long)]
        rows: Option<u16>,
        /// Free-form name to find this session again later (via `list`, or by passing it
        /// directly to send/read/key/resize/close instead of the UUID). Useful for an
        /// agent to identify its own sessions among those of others.
        #[arg(long)]
        label: Option<String>,
    },
    /// Injects text as if typed on the keyboard (id: UUID or label set at open time)
    Send {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        text: String,
        /// Adds a carriage return after the text, as if Enter were pressed right after: avoids
        /// having to write a literal `\r` in `text`, whose escaping depends on the calling
        /// shell (`` `r `` in PowerShell, `$'...\r'` in Bash, never just `\r` inside
        /// single quotes) and which collides with Windows paths containing
        /// `\r`/`\t`/`\n` as a two-character sequence (`C:\repo`, `C:\temp`...).
        #[arg(long)]
        enter: bool,
    },
    /// Sends a command and waits for it to finish (up to --timeout-ms), returning its
    /// output and exit code. No need to guess a delay then re-read with `read`.
    /// Detected via an end-of-command marker injected after the command (syntax adapted to the
    /// session's shell: PowerShell, cmd.exe, or POSIX by default for gitbash/wsl/ssh).
    Exec {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        command: String,
        #[arg(long, default_value_t = 15_000)]
        timeout_ms: u64,
    },
    /// Brings an already-open tab to the foreground, without creating a new one (unlike
    /// `open`). Useful for showing a specific session when several are running in parallel.
    Select {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
    },
    /// Special key: ctrl-c, ctrl-d, ctrl-z, enter, tab, esc
    Key {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        key: String,
    },
    /// Recent output from a session (and its new cursor)
    Read {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        #[arg(long)]
        since: Option<u64>,
        /// Only what happened since the last send/key on this session
        #[arg(long)]
        last: bool,
        /// Strips ANSI sequences (colors, cursor positioning...) from the returned text
        #[arg(long)]
        plain: bool,
    },
    /// Exports a session's entire scrollback to a file
    Export {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        #[arg(long)]
        out: std::path::PathBuf,
        /// Strips ANSI sequences before writing the file
        #[arg(long)]
        plain: bool,
    },
    /// Resizes a session
    Resize {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        cols: u16,
        rows: u16,
    },
    /// Closes a session
    Close {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
    },
    /// Closes every session, without quitting the application (unlike `quit`)
    CloseAll,
    /// Opens a new session with the same parameters as an existing session (same shell,
    /// or same ssh/scp arguments)
    Duplicate {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
    },
    /// Reopens the last closed session (whichever it was), with the same parameters
    Reopen,
    /// Reusable commands stored locally (JSON file, not a secret)
    Snippet {
        #[command(subcommand)]
        action: SnippetAction,
    },
    /// Remote file operations via the system's ssh client (SFTP-equivalent). No
    /// session opened, no secret ever stored. `target` follows the same format as `open --ssh`
    /// (e.g. "kao@axolotl -i C:\path\key").
    Remote {
        #[command(subcommand)]
        action: RemoteAction,
    },
    /// Optional remote web access: view and control sessions from a phone browser over the
    /// network (e.g. Tailscale). Off by default; nothing changes on the machine's network
    /// exposure unless this is explicitly used. Named `web` (not `remote`, already taken by the
    /// unrelated SFTP-equivalent subcommand above).
    Web {
        #[command(subcommand)]
        action: WebAction,
    },
}

#[derive(Subcommand)]
pub enum WebAction {
    /// Starts (or reconfigures) the remote web server on the running window. Reused settings: any
    /// flag left unset falls back to the last saved configuration (bind address), or to a freshly
    /// generated token unless `--no-token` is passed. **You are choosing the network exposure of
    /// this machine's shell**: pick `--bind` deliberately (see the README's `Security` section).
    On {
        /// Address to listen on, e.g. "127.0.0.1:9871" (local only) or "0.0.0.0:9871" (reachable
        /// from the network, e.g. your Tailscale network). No restriction is applied here: the
        /// choice, and its consequences, are entirely yours as this machine's admin.
        #[arg(long)]
        bind: Option<String>,
        /// Use this exact bearer token instead of generating one. Mutually exclusive with
        /// --no-token in intent (if both are passed, --no-token wins).
        #[arg(long)]
        token: Option<String>,
        /// Starts with no authentication at all: anyone who can reach the bind address gets full
        /// shell access, with nothing else to prove. An explicit, informed choice, never the
        /// silent default.
        #[arg(long)]
        no_token: bool,
    },
    /// Stops the remote web server if running.
    Off,
    /// Current state: running or not, bind address, whether a token is configured (never the
    /// token value itself).
    Status,
}

#[derive(Subcommand)]
pub enum RemoteAction {
    /// Lists a remote folder (`ls -la`)
    List { target: String, path: String },
    /// Reads a remote file, writes its raw content to --out (or prints it if absent, losing
    /// any non-printable byte in the process; prefer --out for binary data)
    Read {
        target: String,
        path: String,
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Writes a local file to a remote path
    Write {
        target: String,
        path: String,
        #[arg(long)]
        from: std::path::PathBuf,
    },
    /// Renames/moves a remote file or folder
    Rename {
        target: String,
        from: String,
        to: String,
    },
    /// Deletes a remote file
    Delete { target: String, path: String },
    /// Creates a remote folder (recursive, `mkdir -p`)
    Mkdir { target: String, path: String },
}

#[derive(Subcommand)]
pub enum SnippetAction {
    /// Saves or replaces a snippet
    Add { name: String, text: String },
    /// Lists the saved snippets
    List,
    /// Removes a snippet
    Remove { name: String },
    /// Sends a snippet's text to a session, like a `send`
    Run {
        #[arg(value_name = "ID_OR_LABEL")]
        id: String,
        name: String,
        /// Does not append a newline after the snippet's text (by default, it is
        /// run as if Enter were pressed right after)
        #[arg(long)]
        no_enter: bool,
    },
}

#[derive(Clone, clap::ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

pub fn run(cli: Cli) -> i32 {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let req = match cli.command {
            Command::Snippet { action } => {
                return match action {
                    SnippetAction::Add { name, text } => match crate::snippets::add(&name, &text) {
                        Ok(()) => {
                            println!("ok");
                            0
                        }
                        Err(e) => {
                            eprintln!("beammeup: {e}");
                            1
                        }
                    },
                    SnippetAction::List => {
                        let items = crate::snippets::list();
                        if items.is_empty() {
                            println!("(no snippet)");
                        }
                        for (name, text) in items {
                            println!("{name}\t{text}");
                        }
                        0
                    }
                    SnippetAction::Remove { name } => match crate::snippets::remove(&name) {
                        Ok(()) => {
                            println!("ok");
                            0
                        }
                        Err(e) => {
                            eprintln!("beammeup: {e}");
                            1
                        }
                    },
                    SnippetAction::Run { id, name, no_enter } => {
                        let mut text = match crate::snippets::get(&name) {
                            Ok(t) => t,
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                return 1;
                            }
                        };
                        if !no_enter {
                            text.push('\r');
                        }
                        match ipc::send_request(&Request::Send { id, text }).await {
                            Ok(resp) => {
                                print_response(&resp);
                                if matches!(resp, Response::Error { .. }) {
                                    1
                                } else {
                                    0
                                }
                            }
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                1
                            }
                        }
                    }
                };
            }
            // Bypasses the IPC: these operations never touch the window (no session
            // to open, nothing to display), so they don't need BeamMeUp to be running at all.
            Command::Remote { action } => {
                return match action {
                    RemoteAction::List { target, path } => match crate::remote::list(&target, &path) {
                        Ok(out) => {
                            print!("{out}");
                            0
                        }
                        Err(e) => {
                            eprintln!("beammeup: {e}");
                            1
                        }
                    },
                    RemoteAction::Read { target, path, out } => {
                        match crate::remote::read(&target, &path) {
                            Ok(data) => match out {
                                Some(out_path) => match std::fs::write(&out_path, &data) {
                                    Ok(()) => {
                                        println!("{}", out_path.display());
                                        0
                                    }
                                    Err(e) => {
                                        eprintln!("beammeup: failed to write local file: {e}");
                                        1
                                    }
                                },
                                None => {
                                    print!("{}", String::from_utf8_lossy(&data));
                                    0
                                }
                            },
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                1
                            }
                        }
                    }
                    RemoteAction::Write { target, path, from } => {
                        let content = match std::fs::read(&from) {
                            Ok(c) => c,
                            Err(e) => {
                                eprintln!("beammeup: failed to read local file: {e}");
                                return 1;
                            }
                        };
                        match crate::remote::write(&target, &path, &content) {
                            Ok(()) => {
                                println!("ok");
                                0
                            }
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                1
                            }
                        }
                    }
                    RemoteAction::Rename { target, from, to } => {
                        match crate::remote::rename(&target, &from, &to) {
                            Ok(()) => {
                                println!("ok");
                                0
                            }
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                1
                            }
                        }
                    }
                    RemoteAction::Delete { target, path } => {
                        match crate::remote::delete(&target, &path) {
                            Ok(()) => {
                                println!("ok");
                                0
                            }
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                1
                            }
                        }
                    }
                    RemoteAction::Mkdir { target, path } => {
                        match crate::remote::mkdir(&target, &path) {
                            Ok(()) => {
                                println!("ok");
                                0
                            }
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                1
                            }
                        }
                    }
                };
            }
            Command::Screenshot { out } => {
                // Make sure the window is running (relaunch if needed, like any other command:
                // there is no headless mode) before attempting to capture it.
                if let Err(e) = ipc::send_request(&Request::Status).await {
                    eprintln!("beammeup: {e}");
                    return 1;
                }
                let out =
                    out.unwrap_or_else(|| std::env::temp_dir().join("beammeup-screenshot.png"));
                return match crate::screenshot::capture(&out) {
                    Ok(()) => {
                        println!("{}", out.display());
                        0
                    }
                    Err(e) => {
                        eprintln!("beammeup: screenshot failed: {e}");
                        1
                    }
                };
            }
            Command::Web { action } => {
                return match action {
                    WebAction::On {
                        bind,
                        token,
                        no_token,
                    } => {
                        let resp = match ipc::send_request(&Request::WebOn {
                            bind,
                            token,
                            no_token,
                        })
                        .await
                        {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                return 1;
                            }
                        };
                        match resp {
                            Response::WebStarted { bind, token } => {
                                println!("Remote web access started on http://{bind}/");
                                match token {
                                    Some(token) => println!(
                                        "Token: {token} (save this, it will not be shown again)"
                                    ),
                                    None => println!(
                                        "No token configured: anyone who can reach this address \
                                         has full access."
                                    ),
                                }
                                println!(
                                    "Open http://{bind}/ from your phone (same network / \
                                     Tailscale)."
                                );
                                0
                            }
                            Response::Error { message } => {
                                eprintln!("error: {message}");
                                1
                            }
                            _ => unreachable!(),
                        }
                    }
                    WebAction::Off => match ipc::send_request(&Request::WebOff).await {
                        Ok(resp) => {
                            print_response(&resp);
                            if matches!(resp, Response::Error { .. }) {
                                1
                            } else {
                                0
                            }
                        }
                        Err(e) => {
                            eprintln!("beammeup: {e}");
                            1
                        }
                    },
                    WebAction::Status => {
                        let resp = match ipc::send_request(&Request::WebStatus).await {
                            Ok(r) => r,
                            Err(e) => {
                                eprintln!("beammeup: {e}");
                                return 1;
                            }
                        };
                        match resp {
                            Response::WebStatus {
                                running,
                                bind,
                                token_set,
                            } => {
                                if running {
                                    println!(
                                        "running on {} (token {})",
                                        bind.unwrap_or_default(),
                                        if token_set {
                                            "configured"
                                        } else {
                                            "not configured"
                                        }
                                    );
                                } else {
                                    println!("not running");
                                }
                                0
                            }
                            Response::Error { message } => {
                                eprintln!("error: {message}");
                                1
                            }
                            _ => unreachable!(),
                        }
                    }
                };
            }
            Command::Status => Request::Status,
            Command::List => Request::List,
            Command::Shells => Request::ListShells,
            Command::Quit => Request::Quit,
            Command::Fullscreen { state } => Request::SetFullscreen {
                enabled: matches!(state, OnOff::On),
            },
            Command::Open {
                shell,
                ssh,
                scp,
                cols,
                rows,
                label,
            } => Request::Open {
                shell,
                ssh_args: ssh,
                scp_args: scp,
                cols,
                rows,
                label,
            },
            Command::Send { id, text, enter } => {
                let mut text = text;
                if enter {
                    text.push('\r');
                }
                Request::Send { id, text }
            }
            Command::Exec {
                id,
                command,
                timeout_ms,
            } => {
                let resp = match ipc::send_request(&Request::Exec {
                    id,
                    command,
                    timeout_ms: Some(timeout_ms),
                })
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("beammeup: {e}");
                        return 1;
                    }
                };
                return match resp {
                    Response::Exec {
                        output,
                        exit_code,
                        timed_out,
                    } => {
                        print!("{output}");
                        if timed_out {
                            eprintln!("beammeup: timed out, the command may still be running");
                            124 // POSIX `timeout` convention, for a calling script that checks the exit code
                        } else {
                            exit_code.unwrap_or(0)
                        }
                    }
                    Response::Error { message } => {
                        eprintln!("error: {message}");
                        1
                    }
                    _ => unreachable!(),
                };
            }
            Command::Key { id, key } => Request::Key { id, key },
            Command::Read {
                id,
                since,
                last,
                plain,
            } => Request::Read {
                id,
                since,
                last,
                plain,
            },
            Command::Export { id, out, plain } => {
                let resp = match ipc::send_request(&Request::Read {
                    id,
                    since: Some(0),
                    last: false,
                    plain,
                })
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("beammeup: {e}");
                        return 1;
                    }
                };
                return match resp {
                    Response::Output { data, .. } => match std::fs::write(&out, data) {
                        Ok(()) => {
                            println!("{}", out.display());
                            0
                        }
                        Err(e) => {
                            eprintln!("beammeup: failed to write file: {e}");
                            1
                        }
                    },
                    Response::Error { message } => {
                        eprintln!("error: {message}");
                        1
                    }
                    _ => unreachable!(),
                };
            }
            Command::Resize { id, cols, rows } => Request::Resize { id, cols, rows },
            Command::Select { id } => Request::SelectTab { id },
            Command::Close { id } => Request::Close { id },
            Command::CloseAll => Request::CloseAll,
            Command::Duplicate { id } => Request::DuplicateTab { id },
            Command::Reopen => Request::ReopenLastClosed,
        };

        match ipc::send_request(&req).await {
            Ok(resp) => {
                print_response(&resp);
                if matches!(resp, Response::Error { .. }) {
                    1
                } else {
                    0
                }
            }
            Err(e) => {
                eprintln!("beammeup: {e}");
                1
            }
        }
    })
}

fn print_response(resp: &Response) {
    match resp {
        Response::Ok => println!("ok"),
        Response::Error { message } => eprintln!("error: {message}"),
        Response::Status {
            window_visible,
            session_count,
        } => println!(
            "window open: {window_visible}, active sessions: {session_count}"
        ),
        Response::List { sessions } => {
            if sessions.is_empty() {
                println!("(no session)");
            }
            for s in sessions {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    s.id,
                    s.kind,
                    s.label.as_deref().unwrap_or("-"),
                    s.title,
                    if s.alive { "active" } else { "terminated" }
                );
            }
        }
        Response::Shells { shells } => {
            if shells.is_empty() {
                println!("(nothing detected on this machine)");
            }
            for s in shells {
                println!("{}\t{}\t{}", s.id, s.label, s.program);
            }
        }
        Response::Opened { id } => println!("{id}"),
        Response::Output {
            data,
            cursor,
            alive,
        } => {
            print!("{data}");
            eprintln!("--- cursor={cursor} alive={alive}");
        }
        // Always intercepted before `print_response` (see `Command::Exec`, which needs to
        // reflect the remote exit code into that of the CLI process). Kept here only
        // for the exhaustiveness of the `match`.
        Response::Exec {
            output,
            exit_code,
            timed_out,
        } => {
            print!("{output}");
            eprintln!("--- exit_code={exit_code:?} timed_out={timed_out}");
        }
        Response::Closed { count } => println!("{count} session(s) closed"),
        // Always intercepted before `print_response` in `Command::Web` (which needs to print the
        // token distinctly from the bind address). Kept here only for the exhaustiveness of the
        // `match`.
        Response::WebStarted { bind, token } => {
            println!("started on {bind}, token: {}", token.is_some());
        }
        Response::WebStatus {
            running,
            bind,
            token_set,
        } => {
            println!(
                "running: {running}, bind: {}, token configured: {token_set}",
                bind.as_deref().unwrap_or("-")
            );
        }
    }
}
