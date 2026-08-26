<div align="center">
  <img src="assets/logo-transparent.png" width="180" alt="BeamMeUp logo" />

  # BeamMeUp

  **One terminal window, shared in real time between a human and an AI agent.**

  [![License: BZ-1.1](https://img.shields.io/badge/license-BZ--1.1-8a2be2)](LICENSE)
  ![Platforms: Windows and Linux](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux-0078D6)
</div>

---

## The problem

Coding agents (Claude Code, etc.) run in a sandboxed shell whose standard input is **not a real
terminal**. The result: any interactive tool (arrow key menu, `y/n` confirmation, `eas
credentials` wizard, `npm init`...) crashes with an error along the lines of *"Input is required,
but stdin is not readable"*. The usual workarounds (third party terminals driven through MCP)
work, but stay limited: no fine grained control, no detection of the interpreters actually present
on the machine, no real shared visibility between human and agent.

## The idea

BeamMeUp is **a single application**, with **a single window**, that displays terminal sessions
(PowerShell, cmd, Git Bash, WSL on Windows; bash, zsh, fish on Linux; SSH everywhere) exactly like
a real terminal, except that **two actors can type into it at the same time**: you, on the
keyboard, and an AI agent, through a command line. You see everything the agent does, live, in the
same window you can use yourself.

No server to start ahead of time, no MCP config to write: the agent simply calls the `beammeup`
program, the same way it would call `git` or `npm`. If the window isn't open, it opens itself:
**there is no mode where the program acts without a visible window on screen.**

---

## Features

- 🔍 **Automatic interpreter detection**: on Windows, PowerShell 5.1, PowerShell 7, `cmd.exe`, Git
  Bash and every installed WSL distribution; on Linux, your login shell, then everything declared
  in `/etc/shells` and whatever is found on the `PATH` (bash, zsh, fish...). Nothing is assumed,
  everything is detected on every launch on the real machine (`beammeup shells`).
- 🔐 **SSH through the real system client**: no homegrown SSH library, your `~/.ssh/config`, your
  `known_hosts` and your key agent are used as they are.
- 📤 **File transfer (upload/download)**: through the system's `scp`, visible in a tab like any
  other session.
- 🏷️ **Session labels**: give a session a name so you can find it again later without having to
  memorize a technical identifier. Built so several agents can work in parallel on the same
  machine without stepping on each other.
- ⏱️ **Command completion detection** (`exec`): sends a command, waits for it to actually finish,
  returns its output and exit code. No more guessing a delay and then reading back.
- 📁 **Remote file operations** (`remote`): list/read/write/rename/delete a file on an SSH server
  without opening a session, through the system client.
- 🗂️ **Tab conveniences**: duplicate a session, reopen the last closed one, close everything
  without quitting the application, bring a specific tab to the front (`select`).
- 📋 **Snippets**: save frequently used commands and replay them with a keyword.
- 📄 **Export and clean reading**: pull a session's content into a file, with or without ANSI color
  codes, in full or just "what happened since last time".
- 🖥️ **Remotely controllable full screen**, and **real window screenshot** (Windows only, see
  below).
- 🚫 **No secrets stored**: BeamMeUp remembers no password, key or token. It fully delegates
  authentication to your already configured system tools.
- 🪟 **Standing elevation**: local sessions inherit administrator rights without a prompt
  interrupting every new tab. Through UAC on Windows; through `pkexec` on Linux, and if no
  authentication is possible the window simply starts with your regular rights instead of not
  starting at all.
- 🟢 **Notification area icon**: closing the window (the X button, Alt+F4) hides it without killing
  the running sessions; the icon stays visible next to the clock as long as the program is
  running, with a right click menu (Show / Close all sessions / Quit). `beammeup quit` remains the
  only real way to exit.
- 🎛️ **Vertical sidebar**: designed for shared screen use (agent on the right, BeamMeUp on the
  left): logo, shell picker, session list and snippet management, all reachable with a click,
  without going through the CLI.
- 📱 **Optional remote web access**: view and control sessions from your phone over the network
  (e.g. your Tailscale network). Off by default; you opt in explicitly and choose the bind address
  and authentication yourself (see below and [Security](#security)).

---

## Installation

### Windows

Download the `.msi` or `.exe` installer from the
[latest release](https://github.com/bzhzion/beammeup/releases/latest). Once installed, add the
folder containing `beammeup.exe` (`%LOCALAPPDATA%\beammeup`) to your `PATH` so you can call it
from anywhere.

On the very first launch in window mode, Windows shows a UAC prompt (administrator elevation):
that's expected and intentional (see [Security](#security)).

### Linux

Through the apt repository (Debian/Ubuntu):

```bash
sudo curl -fsSL https://apt.breizhzion.com/KEY.gpg -o /usr/share/keyrings/breizhzion.asc
echo "deb [signed-by=/usr/share/keyrings/breizhzion.asc] https://apt.breizhzion.com stable main" \
  | sudo tee /etc/apt/sources.list.d/breizhzion.list
sudo apt update && sudo apt install beammeup
```

> The key is served ASCII armored and dropped as is in `.asc` format: `apt` can read it directly
> in that format, which avoids depending on `gpg --dearmor` and therefore on the `gnupg` package,
> missing from many minimal installs and container images.

Otherwise, the AppImage from the [latest release](https://github.com/bzhzion/beammeup/releases/latest)
works on any distribution, with no installation needed.

### Building it yourself

Common prerequisites: [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/) (18+). On
Windows, add [Git for Windows](https://git-scm.com/download/win) if you want the Git Bash tab. On
Linux, Tauri's dependencies:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  librsvg2-dev libayatana-appindicator3-dev patchelf build-essential pkg-config
```

```bash
git clone https://github.com/bzhzion/beammeup.git
cd beammeup/app
npm install
npm run tauri build
```

> ⚠️ Always build with `npm run tauri build`, never `cargo build` alone: it's the Tauri CLI that
> embeds the interface into the executable. A plain `cargo build --release` produces a binary
> whose window shows "Could not connect to localhost", while the command line itself works fine,
> which makes the problem easy to miss. The project now refuses to compile in that case, but it's
> worth knowing about.

The packages then end up in `app/src-tauri/target/release/bundle/`.

---

## Full walkthrough

All the commands below follow the same principle: if no BeamMeUp window is running, one opens
automatically before the command runs. You never need to launch it "by hand" beforehand.

### See what's available on your machine

```powershell
beammeup shells
```

Lists the interpreters actually detected, with their identifier, name and path: use that
identifier to open a session.

```
pwsh5     Windows PowerShell 5.1   C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe
cmd       Command Prompt          C:\WINDOWS\System32\cmd.exe
pwsh7     PowerShell 7            C:\Program Files\PowerShell\7\pwsh.exe
gitbash   Git Bash                C:\Program Files\Git\bin\bash.exe
wsl:Debian WSL: Debian            C:\WINDOWS\System32\wsl.exe
```

### Open a session

```powershell
# A local shell, with a name so you can find it again
beammeup open --shell pwsh5 --label my-work

# An SSH connection (reuses your existing config/keys/agent)
beammeup open --ssh "user@myserver.com -i C:\path\to\key" --label prod-server

# A file transfer (upload here; swap the arguments to download)
beammeup open --scp "C:\local\file.txt user@myserver.com:/remote/path/" --label upload
```

Every call to `open` **always creates a new tab**, never silently reuses an existing session. The
window always comes to the front.

### Type into a session

```powershell
# Text, with an Enter right after
beammeup send my-work "npm run build" --enter

# Text without pressing Enter (for example to build up a command over several sends)
beammeup send my-work "npm run "
beammeup send my-work "build" --enter

# A special key
beammeup key my-work ctrl-c
beammeup key my-work enter
```

> ⚠️ Always prefer `--enter` over a literal `\r` in the text: its escaping depends on the calling
> shell (`` `r `` in PowerShell, `$'...\r'` in Bash, impossible in single quotes) and collides with
> Windows paths that contain `\r`/`\t`/`\n` as a two character sequence (`C:\repo`, `C:\temp`...).
> `--enter` sidesteps the problem entirely.

Available keys: `ctrl-c`, `ctrl-d`, `ctrl-z`, `enter`, `tab`, `esc`.

### Read what happened

```powershell
# The full session history, with raw ANSI codes
beammeup read my-work

# Clean version, without color/cursor codes
beammeup read my-work --plain

# Only what happened since the last `send`/`key`
beammeup read my-work --last --plain

# From a specific cursor (returned by previous calls)
beammeup read my-work --since 1024
```

### Run a command and wait for it to finish

```powershell
beammeup exec my-work "npm run build"
```

Unlike `send` (which returns control immediately), `exec` waits for the command to actually
finish, then returns its output and exit code. No more guessing a delay and then rereading with
`read`. Detection relies on a unique marker injected right after the command (syntax adapted to
the session's shell), with a configurable timeout:

```powershell
beammeup exec my-work "Start-Sleep -Seconds 30; ./long-script.ps1" --timeout-ms 60000
```

### Bring a tab to the front

```powershell
beammeup select my-work
```

Unlike `open`, this creates nothing: it just switches the window to an already open tab, handy for
showing a specific session when several are running in parallel.

### Duplicate, reopen, close everything

```powershell
# A new tab with the same settings as an existing one (same shell, or same ssh/scp arguments)
beammeup duplicate my-work

# Closes all sessions WITHOUT quitting the application (unlike `quit`)
beammeup close-all

# Reopens the last closed session (any of them), same settings
beammeup reopen
```

### Files on a remote server (without opening a session)

```powershell
beammeup remote list "user@myserver.com -i C:\path\key" /var/log
beammeup remote read "user@myserver.com" /etc/hostname --out hostname.txt
beammeup remote write "user@myserver.com" /tmp/config.json --from config.json
beammeup remote rename "user@myserver.com" /tmp/old.txt /tmp/new.txt
beammeup remote delete "user@myserver.com" /tmp/new.txt
beammeup remote mkdir "user@myserver.com" /tmp/new-folder
```

A minimal SFTP equivalent, using the system's `ssh` client (the same principle as `--ssh`
sessions): useful for reading/writing a file without going through `send`+`read`, fragile on
binary data or a large file. `target` follows the same format as `open --ssh`. `delete` refuses a
directory (`rm -f` without `-r`) rather than allowing a recursive deletion by mistake.

### Export a session to a file

```powershell
beammeup export my-work --out report.txt --plain
```

### Resize or close

```powershell
beammeup resize my-work 120 40
beammeup close my-work
```

### Snippets: ready to replay commands

```powershell
beammeup snippet add deploy "npm run build && npm run deploy"
beammeup snippet list
beammeup snippet run my-work deploy
beammeup snippet remove deploy
```

Snippets are stored locally (`%LOCALAPPDATA%\beammeup\snippets.json`): this is not a vault, just
plain text commands, so don't put secrets in there.

They can also be managed from the window itself: a dedicated section at the bottom of the sidebar
lists snippets in a dropdown, with a ▶ button to replay one on the active session and
Edit/Add/Delete buttons (deletion requires double confirmation).

### Full screen and screenshots

```powershell
beammeup fullscreen on
beammeup fullscreen off

# Captures a real screenshot of the active tab, writes a PNG
beammeup screenshot --out capture.png
```

### Remote web access

```powershell
# Starts (or reconfigures) the remote web server on the running window, live, no restart needed
beammeup web on --bind 0.0.0.0:9871

# Same, but with an explicit token instead of an auto-generated one
beammeup web on --bind 0.0.0.0:9871 --token my-own-secret

# Same, but with no authentication at all (anyone reaching the address gets full access)
beammeup web on --bind 0.0.0.0:9871 --no-token

# Stops it
beammeup web off

# Running or not, bind address, whether a token is configured (never prints the token itself)
beammeup web status
```

> ⚠️ **This opens your admin-elevated shell to the network.** Off by default; nothing changes on
> this machine's network exposure until you explicitly run `beammeup web on`. There is no
> restriction on the bind address you pass: `0.0.0.0` is accepted exactly as you asked, because
> the choice (and its consequences) is yours as this machine's admin, the same philosophy already
> applied to standing elevation. Pick a bind address you actually intend (a Tailscale IP rather
> than `0.0.0.0` on an untrusted network, for instance).

When neither `--token` nor `--no-token` is passed, a random token is generated and printed once:
save it, it will not be shown again. Settings (bind address, token, and whether to start
automatically on the next launch) are saved to `remote.json` next to `snippets.json`, so a bare
`beammeup web on` afterward reuses the last bind address, and the server comes back automatically
on the next launch unless you run `beammeup web off` first.

Once started, open `http://<bind>/` from your phone's browser (over the same network, or your
Tailscale network if you bound to a Tailscale address): a simple, dark themed page lets you pick a
session, watch its output (polled every 1.5 seconds, not a live stream: see
[Security](#security)), and send text to it. If a token is configured, the page asks for it once
and remembers it for the browser tab's lifetime only (`sessionStorage`, cleared when the tab
closes), never `localStorage`.

### List open sessions

```powershell
beammeup list
beammeup status
```

### Cleanly close the application

```powershell
beammeup quit
```

With BeamMeUp running elevated (administrator), an ordinary `Stop-Process` fails with *"Access is
denied"* even for its own user: `quit` is the only reliable way to close it from the outside.

Closing the window (the X button, Alt+F4) does **not** quit the application: the sessions live in
the same process, so killing it along with the window would lose them all. The window simply hides
and the program keeps running, signaled by the icon in the notification area. Clicking it (or
"Show" in its menu) reopens the window, and any CLI command (`open`, `select`...) brings it back
automatically too.

---

## For AI agents

This section is for any agent (Claude Code, ChatGPT/Codex, or otherwise) driving BeamMeUp, not
just the human installing the program. `beammeup --help` and `beammeup <subcommand> --help` remain
the exact source of truth (always in sync with the installed binary); what follows is context that
the built-in help can't provide.

- **Never a headless mode.** Every command relaunches the window if needed and always makes it
  appear visible on screen. This isn't a limitation to work around: it's the guarantee that the
  human always sees what the agent is doing, in the same window they're using.
- **Prefer `exec` over `send` for any command whose result you're waiting on.** `send` returns
  control immediately (useful for interactive typing, answering a prompt); `exec` waits for the
  command to actually finish and returns output plus exit code. Guessing a delay and then rereading
  with `read` is a technique left over from before `exec` existed, no longer needed today.
- **Known, unresolved limitation: the very first character sent to a brand new session can be
  lost**, on Windows, if the first send arrives after a period of silence. Practical workaround:
  on a session that was just opened, send a throwaway character (for example a space) before the
  real command, or simply check the result with `read`/`exec` and resend if the beginning is
  missing.
- **Labels rather than UUIDs.** `open --label my-name`, then reusing `my-name` everywhere (`send`,
  `read`, `exec`, `close`...) is more readable and more robust than a UUID copied by hand,
  especially when several agents or several sessions are running in parallel on the same machine.
  If a label is duplicated, resolution always picks the most recent session.
- **One window per machine, one control channel.** All commands from a given machine talk to the
  same instance; there's no notion of an isolated "BeamMeUp session" per agent. Two agents working
  in parallel on the same machine share the same tab list; `select` lets one of them bring the tab
  they care about to the front without disturbing the others.
- **`close-all` is not `quit`.** `close-all` empties the tabs but leaves the window open; `quit`
  also closes the window and is the only reliable way to stop it from the outside (an elevated
  process refuses a non elevated `Stop-Process`/`taskkill`).
- **`remote` doesn't go through any session.** To read/write a file on an already configured SSH
  server, `beammeup remote read|write|list|rename|delete|mkdir` is more direct and more reliable on
  binary data or a large file than a `send`+`read` round trip inside a shell session.

---

## How it works (in short)

One executable, two behaviors:

- **With no argument** → window mode: opens the interface, the session manager, and a local
  control channel (Windows named pipe).
- **With a subcommand** (`open`, `send`, `read`...) → connector mode: connects to the control
  channel; if the window doesn't exist yet, relaunches it automatically (visibly), waits for it to
  be ready, then forwards the command.

Each session is a real pseudo-terminal (ConPTY on Windows) running either a locally detected
interpreter or the system's `ssh`/`scp` client. The window and the external connector act on
exactly the same process: whatever one types, the other sees.

## Security

- **No secrets stored.** BeamMeUp knows no password, private key or token: everything goes through
  the tools already configured on your machine (SSH agent, `known_hosts`, etc.).
- **Standing elevation, by design.** The program runs as administrator so local sessions inherit
  its rights without a repeated prompt. This is a deliberate tradeoff for a personal,
  single-user tool, not suited to a shared machine.
- **Local control channel only.** The command pipe and the screenshot capture port only listen
  locally (`127.0.0.1`/named pipe), never reachable over the network. The command pipe is
  restricted by ACL to the Windows account that launched the application, and only accepts
  connections from the BeamMeUp executable itself. The screenshot port (CDP), however, remains
  reachable by any process running on the machine regardless of its Windows account (a limitation
  of the protocol, not of the DACL): an accepted tradeoff for a personal, single-user machine, not
  suited to a machine shared between several accounts.
- **Remote web access is the one deliberate exception to "local only", and it is opt-in.** Running
  `beammeup web on` starts a real HTTP server reachable over the network, on whatever bind address
  you choose, including `0.0.0.0`: this module does not restrict or refuse any address, the same
  tradeoff already accepted for standing elevation, because the choice belongs to you as this
  machine's admin, not to the program. Concretely, once started:
  - The bind address and whether a token is required are entirely your choice; a safe default
    (a randomly generated token) is used only when you pass neither `--token` nor `--no-token`,
    never a silent "no auth" default.
  - The token is compared with a constant-time comparison, never a plain `==`, to avoid a timing
    side channel.
  - There is **no rate limiting on token attempts** in this first version: an attacker who can
    reach the bind address can try tokens as fast as the network allows. Accepted tradeoff for now
    (a random 128-bit token is not brute-forceable in practice over a network round trip, but this
    is a real gap compared to a production authentication system), consistent with the rest of
    this section's approach of documenting risk rather than pretending it away.
  - No WebSocket, HTTP polling only: a deliberate scope decision to keep this networked code small
    and auditable.
  - The endpoints give full read/write access to every open session (including whatever an
    elevated shell can do): treat the token exactly like the password to this machine's shell,
    because that is functionally what it is.
- See the [issues](https://github.com/bzhzion/beammeup/issues) for known limitations.

## License

[BZ-1.1](LICENSE): BREIZHZION Personal Use License. Personal use only; manufacturing or commercial
use for a third party is prohibited without a written commercial license. See the full text in
[`LICENSE`](LICENSE).
