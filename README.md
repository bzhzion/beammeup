<div align="center">
  <img src="assets/logo-transparent.png" width="180" alt="BeamMeUp logo" />

  # BeamMeUp

  **One terminal window, shared in real time between a human and an AI agent.**

  [![License: BZ-1.1](https://img.shields.io/badge/license-BZ--1.1-8a2be2)](LICENSE.md)
  ![Platforms: Windows and Linux](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux-0078D6)
</div>

---

Coding agents run in a sandboxed shell that is **not a real terminal**, so anything interactive
(arrow-key menus, `y/n` prompts, wizards like `npm init`) fails on them.

BeamMeUp is a single window showing real terminal sessions (PowerShell, cmd, Git Bash, WSL, bash,
zsh, fish, SSH) that **two actors can type into at the same time**: you on the keyboard, and an
agent through the `beammeup` command. You see everything the agent does, live. There is no server
to start and no MCP to configure: if the window isn't open, the first command opens it.

## Features

- **Detects the shells actually installed** on the machine (`beammeup shells`).
- **SSH and SCP through your system client**: your `~/.ssh/config`, `known_hosts` and key agent,
  untouched. BeamMeUp stores no password or key.
- **Session labels**, so several agents can work side by side without colliding.
- **`exec`** runs a command and waits for it to finish, returning its output and exit code.
- **Remote file operations** over SSH without opening a session.
- **Snippets**, session export, full screen, window screenshots.
- **Runs elevated**: local sessions get administrator rights without a prompt on every tab.
- **Stays in the notification area** when you close the window, so sessions survive.
- **Optional remote web access** from your phone, off by default.

## Installation

### Windows

```powershell
winget install Breizhzion.BeamMeUp
```

Or download the installer from the
[latest release](https://github.com/bzhzion/beammeup/releases/latest). Windows asks for
administrator rights on first launch: that is expected (see [Security](#security)).

### Linux (Debian/Ubuntu)

```bash
sudo curl -fsSL https://apt.breizhzion.com/KEY.gpg -o /usr/share/keyrings/breizhzion.asc
echo "deb [signed-by=/usr/share/keyrings/breizhzion.asc] https://apt.breizhzion.com stable main" \
  | sudo tee /etc/apt/sources.list.d/breizhzion.list
sudo apt update && sudo apt install beammeup
```

The AppImage from the [latest release](https://github.com/bzhzion/beammeup/releases/latest) works
on any distribution.

### Building from source

Requires [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/) 18+, plus
[Tauri's dependencies](https://v2.tauri.app/start/prerequisites/) on Linux.

```bash
git clone https://github.com/bzhzion/beammeup.git
cd beammeup/app
npm install
npm run tauri build
```

Use `npm run tauri build`, not `cargo build`: only the former embeds the interface.

## Usage

`beammeup --help` and `beammeup <command> --help` are always up to date with your version.

### Sessions

```powershell
beammeup shells                                         # available shells and their ids
beammeup open --shell pwsh7 --label work                # local shell
beammeup open --ssh "user@server -i C:\path\key" --label prod
beammeup open --scp "C:\file.txt user@server:/tmp/" --label upload
beammeup list                                           # open sessions
beammeup select work                                    # switch the window to this tab
beammeup duplicate work
beammeup close work
beammeup close-all                                      # closes every tab, keeps the window
beammeup reopen                                         # reopens the last closed tab
beammeup quit                                           # really exits
```

`open` always creates a new tab. If two sessions share a label, the most recent one wins.

### Typing and reading

```powershell
beammeup send work "npm run build" --enter
beammeup key work ctrl-c
beammeup key work down
beammeup exec work "npm test" --timeout-ms 60000        # waits, returns output + exit code
beammeup read work --last --plain                       # output since the last send, no colors
beammeup export work --out log.txt --plain
```

Prefer `--enter` to a literal `\r`, whose escaping depends on your shell.

`key` accepts `enter`, `tab`, `esc`, `space`, `backspace`, `delete`, `insert`, the arrows, `home`,
`end`, `pageup`, `pagedown`, `f1` to `f12`, `ctrl-<letter>`, the `ctrl-`, `alt-` and `shift-`
modifiers, and any single character. Version 1.0.4 and older only know `ctrl-c`, `ctrl-d`,
`ctrl-z`, `enter`, `tab` and `esc`.

### Remote files over SSH

```powershell
beammeup remote list   "user@server" /var/log
beammeup remote read   "user@server" /etc/hostname --out hostname.txt
beammeup remote write  "user@server" /tmp/config.json --from config.json
beammeup remote rename "user@server" /tmp/a.txt /tmp/b.txt
beammeup remote delete "user@server" /tmp/b.txt         # refuses directories
beammeup remote mkdir  "user@server" /tmp/folder
```

### Snippets, screen and window

```powershell
beammeup snippet add deploy "npm run build && npm run deploy"
beammeup snippet run work deploy
beammeup snippet list
beammeup snippet remove deploy
beammeup resize work 120 40
beammeup fullscreen on
beammeup screenshot --out capture.png
```

Snippets are stored as plain text: don't put secrets in them. They can also be managed from the
sidebar.

### Remote web access

```powershell
beammeup web on --bind 100.x.x.x:9871      # prints a generated token, once
beammeup web status
beammeup web off
```

Then open `http://<bind>/` on your phone to watch and type into sessions. The setting survives a
restart until you run `web off`. Read [Security](#security) before using it.

## Tips for AI agents

- **Use `exec`** when you need a result, `send` only to answer a prompt or type interactively.
- **Pick one label that identifies you** and reuse it for your whole task.
- **The window is always visible but never steals focus**, `select` included.
- **On a brand new Windows session, the very first character may be lost.** Send a throwaway space
  first, or check with `read` and resend.
- **Stop BeamMeUp with `beammeup quit`**: it runs elevated, so `Stop-Process` is refused.

## Security

- **Runs as administrator by design**, so every local session is elevated. Meant for a personal,
  single-user machine.
- **Local control only.** The command channel is restricted to your Windows account and to the
  BeamMeUp executable. The screenshot port listens on `127.0.0.1` but is reachable by any local
  process: avoid machines shared between several accounts.
- **Remote web access opens your elevated shell to the network.** It is off until you run
  `beammeup web on`, and the bind address is entirely your choice: prefer a Tailscale IP, never
  `0.0.0.0` on a network you don't trust. It is plain HTTP, so the token and everything on screen
  travel unencrypted outside an encrypted network such as Tailscale. Treat the token like this
  machine's root password. `--no-token` gives full access to anyone who can reach the address.
- **The only secret stored is that token**, in `remote.json` (`%APPDATA%\beammeup\` on Windows,
  `~/.config/beammeup/` on Linux). `web off` keeps it; delete the file to remove it.

Known limitations are tracked in the [issues](https://github.com/bzhzion/beammeup/issues).

## License

[BZ-1.1](LICENSE.md): BREIZHZION Personal Use License. Personal use only; manufacturing or commercial
use for a third party is prohibited without a written commercial license.
