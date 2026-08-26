// Kept on the "console" subsystem by default (no windows_subsystem="windows"): this is what lets
// the CLI connector mode keep a normal stdin/stdout, inherited as-is from the caller
// (Bash/PowerShell on the agent side), without juggling AttachConsole. An attempt to switch to the
// "windows" subsystem plus `AttachConsole(ATTACH_PARENT_PROCESS)` was tried on 2026-08-25: it
// silently breaks output/exit code capture in an automated environment where the process is
// already invoked with explicitly redirected handles (instead of inheriting a real console), too
// risky for a purely cosmetic gain. Window mode instead hides its console after the fact with
// `FreeConsole()`, without touching CLI mode.
use clap::Parser;

/// Detaches the console automatically allocated in window mode, so as not to leave an empty black
/// rectangle behind the interface. Has no effect in CLI mode (never called on that branch): this
/// is precisely what distinguishes this approach from a global `windows_subsystem="windows"`.
#[cfg(windows)]
fn detacher_console_fenetre() {
    unsafe {
        let _ = windows::Win32::System::Console::FreeConsole();
    }
}

/// Guard against a pitfall hit twice: a bare `cargo build --release` produces a binary that looks
/// for the development server instead of the embedded assets, and the window shows "Could not
/// connect to localhost: Connection refused". It compiles, it launches, the CLI and IPC work
/// perfectly, and only a visual inspection reveals that the interface is empty.
///
/// `tauri::is_dev()` is `const`: the condition is therefore resolved at compile time, at no
/// runtime cost. `compile_error!` cannot be used on a feature of our own crate, because the Tauri
/// CLI enables `custom-protocol` directly on the `tauri` crate without going through us (tried,
/// and it blocked the legitimate build).
///
/// Deliberately limited to the release profile: in development, pointing at the Vite server is
/// exactly what we want.
#[cfg(not(debug_assertions))]
const _: () = assert!(
    !tauri::is_dev(),
    "Release build in development mode: the binary would load the development server instead \
     of the embedded interface, and the window would show a connection error. Use \
     `npm run tauri build` (from app/) instead of `cargo build --release`."
);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        // CLI connector mode: never elevated, to keep a normal stdin/stdout redirection (see
        // app.manifest, a requireAdministrator manifest would break this for everyone).
        let cli = beammeup_lib::Cli::parse();
        std::process::exit(beammeup_lib::run_cli(cli));
    }

    // Window mode: if an instance is already running, bring it to the foreground and stop instead
    // of building a second, independent and uncoordinated window (see this function's doc, a bug
    // found and fixed on 2026-08-25).
    if beammeup_lib::another_instance_is_running_and_focused() {
        std::process::exit(0);
    }

    // Window mode: no need for the console inherited from the subsystem, it would only show up
    // empty behind the Tauri window.
    #[cfg(windows)]
    detacher_console_fenetre();

    // Window mode: elevation is handled here, not in the manifest (see elevation.rs).
    //
    // `relance_deja_tentee()` is the guard against the infinite relaunch loop found on
    // 2026-08-25 (see its documentation): on a machine where UAC is disabled, `is_elevated()` can
    // be wrong and stay "not elevated" even after a successful relaunch, which without this guard
    // would trigger another relaunch endlessly.
    #[cfg(windows)]
    if !beammeup_lib::is_elevated() && !beammeup_lib::relance_deja_tentee() {
        // Never returns: either the elevated relaunch takes off, or UAC is denied and we stop.
        beammeup_lib::relaunch_elevated_and_exit();
    }

    // On Unix, a refusal is not fatal: we continue with a non-elevated window rather than leaving
    // the user with nothing (a user terminal remains perfectly usable there, unlike Windows where
    // waiting for an admin shell is structural).
    #[cfg(unix)]
    if !beammeup_lib::is_elevated() {
        beammeup_lib::try_relaunch_elevated();
    }

    beammeup_lib::run();
}
