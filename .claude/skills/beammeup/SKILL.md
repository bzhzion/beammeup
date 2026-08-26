---
name: beammeup
description: >
  Drive BeamMeUp (`beammeup.exe`/`beammeup`), the application that gives the agent and the human
  a shared, real-time terminal window (PowerShell/cmd/Git Bash/WSL/bash/SSH). Use it whenever a
  task needs a real interactive terminal that the agent's sandboxed shell cannot provide (arrow-key
  menus, wizards like `eas credentials`/`npm init`, `y/n` confirmations, a password prompt the
  human needs to see), or simply when the user explicitly asks to use BeamMeUp. This skill is
  versioned in the `bzhzion/beammeup` repository: update it in the same commit as any CLI change,
  not as an afterthought.
---

# BeamMeUp: driving the shared terminal

BeamMeUp is a single executable with two behaviors: run it with no argument and it opens the
window (interface + session manager); run it with a subcommand and it acts as a connector that
talks to the already-open window (and relaunches it automatically if needed). **There is no
headless mode**: every command makes a window appear or reappear on screen, never an action
invisible to the human.

`beammeup --help` and `beammeup <subcommand> --help` are the exact source of truth, always in
sync with the binary actually installed (see the end of this skill for how to consult them when
the PATH isn't configured). This skill provides the context the built-in help cannot: when to use
what, and the pitfalls already encountered.

## Basic workflow

1. **Discover the available shells** (never assume a list in advance, each machine differs):
   ```
   beammeup shells
   ```
2. **Open a session with an explicit label** (reuse it everywhere afterward, more readable and
   more reliable than a hand-copied UUID):
   ```
   beammeup open --shell pwsh5 --label mon-travail
   beammeup open --ssh "user@host -i C:\chemin\cle" --label serveur-prod
   ```
   `open` always creates a new tab, never a silent reuse of an existing one.
3. **Run a command and wait for its result**: prefer `exec` over `send` whenever a command
   produces a result you need to capture:
   ```
   beammeup exec mon-travail "npm run build"
   ```
   Returns output plus exit code once the command has actually finished (detected via an
   injected end marker, with syntax adapted to the shell). Use `--timeout-ms` for long-running
   commands.
4. **`send`/`key` remain useful for interactive work** (free-form typing, answering a prompt, a
   special key):
   ```
   beammeup send mon-travail "npm run build" --enter
   beammeup key mon-travail ctrl-c
   ```
   Always use `--enter` rather than a literal `\r` (escaping differs by calling shell, and it can
   collide with Windows paths like `C:\repo`, `C:\temp`...).
5. **Read without running a new command**:
   ```
   beammeup read mon-travail --plain --last
   ```
6. **Close cleanly**: `beammeup close mon-travail` (one session), `beammeup close-all` (all
   sessions, window stays open), `beammeup quit` (closes everything, including the window: the
   only reliable way to stop it from the outside, since an elevated process refuses a
   non-elevated `Stop-Process`/`taskkill`).

## Other useful commands

- `beammeup select <id_or_label>` brings an already-open tab to the foreground without creating a
  new one, useful when several sessions are running in parallel.
- `beammeup duplicate <id_or_label>` opens a new tab with the same settings as an existing one.
- `beammeup reopen` reopens the last closed session (whichever it was), with the same settings.
- `beammeup remote list|read|write|rename|delete|mkdir <target> <path>` performs file operations
  on an SSH server **without opening a session** (a minimal SFTP equivalent via the system `ssh`
  client): more direct and more reliable than a `send`+`read` round trip for binary content or a
  large file. `target` follows the same format as `open --ssh`.
- `beammeup snippet add|list|run|remove` manages reusable commands stored locally (not a secret,
  just plain text).
- `beammeup export <id_or_label> --out fichier.txt --plain` exports the whole scrollback to a
  file.
- `beammeup web on --bind <addr> [--token <t> | --no-token]` / `beammeup web off` / `beammeup web
  status` turn the optional remote web server on/off or report its state, so sessions can be
  viewed/controlled from a phone over the network. **Off by default.** Never enable this on the
  user's behalf without being explicitly asked: it changes the machine's network exposure (an
  admin-elevated shell reachable over the network), and the bind address/token are the user's
  informed choice to make, not the agent's.

## Known pitfalls

- **Unresolved limitation: the very first character sent to a fresh session can be lost.** On
  Windows, this happens if the first send arrives after a period of silence: a ConPTY/PSReadLine
  bug confirmed empirically, including through fix attempts that were abandoned after they
  corrupted the display while typing was in progress (see the git history of
  `app/src-tauri/src/session.rs`). Practical workaround: on a session that was just opened, send a
  throwaway character before the real command, or check the result with `read`/`exec` and resend
  if the beginning is missing.
- **One window per machine, one control channel.** All commands from a given machine talk to the
  same instance; two agents running in parallel on the same machine share the same tab list.
- **`close-all` is not `quit`.** The first empties the tabs, the second also closes the window.
- **Closing the window (the X button) does not quit the program.** It simply hides it (an icon
  stays visible in the notification area, sessions remain alive). Don't conclude the program has
  stopped just because the window no longer appears: trust `beammeup status`, not the visual
  state. Only `quit` actually terminates the process.
- **PATH not configured.** If `beammeup` isn't on the agent's PATH, use the full path
  (`%LOCALAPPDATA%\beammeup\beammeup.exe` on Windows) rather than assuming a global install.
