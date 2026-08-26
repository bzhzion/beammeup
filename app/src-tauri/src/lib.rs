mod cli;
mod commands;
mod elevation;
mod ipc;
mod protocol;
mod remote;
mod remote_web;
mod screenshot;
mod session;
mod shells;
mod snippets;

use std::sync::Arc;

use session::{DataEvent, SessionManager};
use tauri::{Emitter, Manager};

pub use cli::{run as run_cli, Cli, Command};
pub use ipc::another_instance_is_running_and_focused;
pub use elevation::is_elevated;
#[cfg(windows)]
pub use elevation::relaunch_elevated_and_exit;
#[cfg(windows)]
pub use elevation::relance_deja_tentee;
#[cfg(unix)]
pub use elevation::try_relaunch_elevated;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let manager = Arc::new(SessionManager::new());
    let remote_web_handle = Arc::new(remote_web::RemoteWebHandle::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(manager.clone())
        .invoke_handler(tauri::generate_handler![
            commands::ui_list,
            commands::ui_shells,
            commands::ui_version,
            commands::ui_open,
            commands::ui_send,
            commands::ui_key,
            commands::ui_resize,
            commands::ui_close,
            commands::ui_read,
            commands::ui_set_fullscreen,
            commands::ui_snippet_list,
            commands::ui_snippet_add,
            commands::ui_snippet_remove,
        ])
        .setup(move |app| {
            // Window created manually (instead of the automatic creation via tauri.conf.json,
            // disabled with "create": false) so we can enable WebView2's remote debug port: the
            // standard WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS environment variable does NOT survive
            // UAC elevation (Windows rebuilds the elevated process's environment from a cached copy
            // of the session, not from the calling process, confirmed by testing; no scope,
            // including the persistent registry, worked around it). Passing the argument directly
            // to the webview builder sidesteps the problem.
            //
            // Active in both debug AND release builds (see `screenshot.rs`, `beammeup screenshot`):
            // this lets the agent capture a real screenshot of the window on demand, without
            // depending on an OS screen capture (unavailable in its sandboxed environment).
            //
            // SECURITY: `--remote-allow-origins=*` was removed here (found during the audit on
            // 2026-08-24). This flag disabled the origin check that Chromium applies by default on
            // WebSocket connections to the CDP debug port. Combined with BeamMeUp's admin
            // elevation, any web page open in any browser on the same machine could have connected
            // via JavaScript to this port and executed arbitrary code in the window (thus calling
            // the Tauri commands `ui_open`/`ui_send` with admin rights). Without this flag, Chromium
            // rejects any WebSocket request carrying a browser-style `Origin` header; our own
            // capture client (`screenshot.rs`) speaks raw TCP and never sets this header, so it is
            // not affected by this removal.
            // Residual risk accepted (single-user machine, see README `## Security`): the port
            // remains locally accessible to any process that speaks the CDP protocol directly
            // without going through a browser (no origin check is possible in that case), the same
            // risk category as the control named pipe on a machine shared between multiple Windows
            // accounts.
            let window_config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .expect("'main' window missing from tauri.conf.json")
                .clone();
            let window_builder = tauri::WebviewWindowBuilder::from_config(app, &window_config)?;
            // `additional_browser_args` is not gated by a `cfg` in Tauri, but it is only read by
            // the WebView2 backend: on Linux (WebKitGTK) it is a silent no-op. We restrict it
            // explicitly to Windows so the code is honest about what it actually does, rather than
            // implying a debug port is opened everywhere.
            #[cfg(windows)]
            let window_builder = window_builder.additional_browser_args(&format!(
                "--remote-debugging-port={}",
                screenshot::CDP_PORT
            ));
            let window = window_builder.build()?;

            // Closing the window (X button, Alt+F4) hides it instead of quitting the application:
            // the PTY sessions live in this same process, so destroying it would kill all of them
            // along with it. `beammeup quit` remains the only real way out, but it also goes
            // through a `WindowEvent::CloseRequested` on this window (confirmed by testing on
            // 2026-08-26, contrary to what the previous comment assumed): without the `exiting`
            // flag below to distinguish the two cases, `prevent_close()` was blocking `quit` itself
            // indefinitely. The process stayed alive and fully functional after responding "ok",
            // never detected because a full launch/close cycle had never been tested until then.
            // Any command that comes afterwards (`open`/`send`/`select`...) already calls
            // `show_and_focus`, so the window reappears normally after being hidden. Accepted
            // trade-off: the application can run with no visible window between an accidental close
            // and the next command. Not a real headless mode (no action is ever taken while the
            // window is hidden, only already-open sessions keep living in the background), but this
            // is worth watching in case it becomes misleading for the human (a taskbar indicator is
            // a possible future addition).
            let exiting = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let window_for_close = window.clone();
            let exiting_for_close = exiting.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if !exiting_for_close.load(std::sync::atomic::Ordering::SeqCst) {
                        api.prevent_close();
                        let _ = window_for_close.hide();
                    }
                }
            });

            // The final title is set on the frontend side (see `ui_version` and `main.ts`): a call
            // here would get overwritten anyway by the static `<title>` tag in the HTML once the
            // page loads (confirmed on 2026-08-25: the installed window showed "BeamMeUp" without a
            // version despite this call).

            let app_handle = app.handle().clone();
            let emit_handle = app_handle.clone();
            manager.set_emitter(Arc::new(move |ev: DataEvent| {
                let _ = emit_handle.emit("session-data", ev);
            }));
            let opened_handle = app_handle.clone();
            manager.set_opened_emitter(Arc::new(move |info| {
                let _ = opened_handle.emit("session-opened", info);
            }));
            let closed_handle = app_handle.clone();
            manager.set_closed_emitter(Arc::new(move |id| {
                let _ = closed_handle.emit("session-closed", id);
            }));
            let selected_handle = app_handle.clone();
            manager.set_selected_emitter(Arc::new(move |id| {
                let _ = selected_handle.emit("select-tab", id);
            }));

            let focus_handle = app_handle.clone();
            let show_and_focus = Arc::new(move || {
                if let Some(window) = focus_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            });

            // The only clean way to close BeamMeUp from the outside: an elevated process cannot be
            // killed by a non-elevated `taskkill`/`Stop-Process` (`Access is denied`, confirmed by
            // testing). `beammeup quit` goes through this same API rather than an OS signal that
            // the agent could not send anyway.
            let quit_handle = app_handle.clone();
            let exiting_for_quit = exiting.clone();
            let quit = Arc::new(move || {
                exiting_for_quit.store(true, std::sync::atomic::Ordering::SeqCst);
                quit_handle.exit(0);
            });

            let fullscreen_handle = app_handle.clone();
            let set_fullscreen = Arc::new(move |enabled: bool| {
                if let Some(window) = fullscreen_handle.get_webview_window("main") {
                    let _ = window.set_fullscreen(enabled);
                }
            });

            // Icon in the notification area (next to the clock): the only visual indicator that
            // the application is still running once the window is hidden (see the comment on
            // `on_window_event` above). Without it, nothing distinguishes "closed for good" from
            // "hidden in the background" to the eye; only `beammeup status` revealed it until now.
            let tray_show_menu = show_and_focus.clone();
            let tray_show_click = show_and_focus.clone();
            let tray_quit = quit.clone();
            let tray_manager = manager.clone();
            let show_item =
                tauri::menu::MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let close_all_item = tauri::menu::MenuItem::with_id(
                app,
                "close-all",
                "Close all sessions",
                true,
                None::<&str>,
            )?;
            let quit_item =
                tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu =
                tauri::menu::Menu::with_items(app, &[&show_item, &close_all_item, &quit_item])?;
            tauri::tray::TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("default icon").clone())
                .tooltip("BeamMeUp")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |_app, event| match event.id.as_ref() {
                    "quit" => tray_quit(),
                    "show" => tray_show_menu(),
                    // Does not bring the window back up: this is precisely meant to let sessions be
                    // cleared without having to show the window, straight from the notification area.
                    "close-all" => {
                        tray_manager.close_all();
                    }
                    _ => {}
                })
                .on_tray_icon_event(move |_tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        tray_show_click();
                    }
                })
                .build(app)?;

            let controls = ipc::WindowControls {
                show_and_focus,
                quit,
                set_fullscreen,
                remote_web: remote_web_handle.clone(),
            };

            // Optional remote web access: only starts here if the user previously ran `beammeup
            // web on` (which persists `autostart: true`, see `remote_web.rs` and `ipc.rs`'s
            // `Request::WebOn` handling) and hasn't since run `beammeup web off` (which persists
            // `autostart: false`). Off by default, exactly like everything else in this feature.
            // A stale or now-invalid saved bind address must never prevent the whole app from
            // launching, hence a plain `eprintln!` instead of a panic.
            let remote_web_config = remote_web::load_config();
            if remote_web_config.autostart {
                if let Err(e) = remote_web_handle.start(
                    manager.clone(),
                    remote_web_config.bind.clone(),
                    remote_web_config.token.clone(),
                ) {
                    eprintln!("beammeup: failed to autostart the remote web server ({e})");
                }
            }

            let ipc_manager = manager.clone();
            tauri::async_runtime::spawn(async move {
                ipc::serve(ipc_manager, controls).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
