// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Atlas desktop shell: a native window on the factory and an icon in the menu bar or tray.
//!
//! The shell carries the front; the factory itself lives in the daemon, a separate process the
//! shell starts when none runs and whose address and token it hands to its own page only.
//! Closing the window hides it; the daemon and its agents carry on until asked to stop. On Linux,
//! where a desktop may show no tray to bring it back, closing the window quits the shell instead.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod daemon;

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use atlas_i18n::{Language, tr};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const MAIN_WINDOW: &str = "main";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest the shell waits for a daemon it asked to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Version of the daemon this shell ships: both are built from the same workspace.
const SHIPPED_VERSION: &str = env!("CARGO_PKG_VERSION");
const MENU_OPEN: &str = "open";
const MENU_QUIT_SHELL: &str = "quit-shell";
const MENU_STOP_DAEMON: &str = "stop-daemon";

/// Held while the daemon is being started, so two callers do not start two.
struct Startup(Mutex<()>);

/// What the page needs to call the daemon: nothing else on this machine is told the token.
#[derive(Serialize)]
struct DaemonConnection {
    url: String,
    token: String,
}

/// Makes sure a daemon runs, starting one when none answers, and tells how to call it.
fn connect(app: &AppHandle) -> Result<DaemonConnection, String> {
    let startup = app.state::<Startup>();
    let _starting = startup.0.lock().unwrap_or_else(PoisonError::into_inner);
    let home = atlas_link::atlas_home();
    let port = atlas_link::configured_port(&home);
    let startup = daemon::startup_for(atlas_link::running_version(port).as_deref(), SHIPPED_VERSION);
    if startup == daemon::StartupPlan::Replace {
        // Its agents lose their sessions: the new daemon shows them stopped, ready to be resumed.
        atlas_link::stop(port, &home).map_err(|error| {
            tr!(
                "Le daemon d'une autre version d'Atlas n'a pas pu être arrêté : {error}",
                "The daemon of another version of Atlas could not be stopped: {error}"
            )
        })?;
        if !daemon::wait_until_stopped(port, STOP_TIMEOUT) {
            return Err(tr!(
                "Le daemon d'une autre version d'Atlas répond encore sur le port {port}.",
                "The daemon of another version of Atlas still answers on port {port}."
            ));
        }
    }
    if startup != daemon::StartupPlan::Reuse {
        let mut child = daemon::daemon_binary()
            .and_then(|binary| daemon::spawn_detached(&binary, &home))
            .map_err(|error| {
                tr!(
                    "Le daemon n'a pas pu démarrer : {error}",
                    "The daemon could not start: {error}"
                )
            })?;
        let is_running = daemon::wait_until_running(port, &mut child, STARTUP_TIMEOUT);
        // Reaps the daemon if it ends before the shell; it is never killed from here.
        std::thread::spawn(move || child.wait());
        if !is_running {
            return Err(tr!(
                "Le daemon ne répond pas sur le port {port}.\nJournal : {}",
                "The daemon does not answer on port {port}.\nLog: {}",
                daemon::log_path(&home).display()
            ));
        }
    }
    let token = atlas_link::read_token(&home).map_err(|error| {
        tr!(
            "Jeton du daemon illisible : {error}",
            "Token of the daemon unreadable: {error}"
        )
    })?;
    Ok(DaemonConnection {
        url: atlas_link::daemon_url(port),
        token,
    })
}

/// Called by the page when it loads, and again when the user retries after a failed start.
#[tauri::command]
async fn connect_to_daemon(app: AppHandle) -> Result<DaemonConnection, String> {
    tauri::async_runtime::spawn_blocking(move || connect(&app))
        .await
        .map_err(|error| error.to_string())?
}

/// Brings the window back; a daemon stopped meanwhile is started again, and the page finds it by itself.
fn show_factory(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    if !atlas_link::is_running(atlas_link::configured_port(&atlas_link::atlas_home())) {
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(error) = connect(&app) {
                eprintln!("[atlas] {error}");
            }
        });
    }
}

/// Stops the daemon, then quits. A daemon that would not stop keeps the shell open on the factory:
/// quitting would tell the master that his agents stopped while they carry on.
fn stop_daemon_and_quit(app: &AppHandle) {
    let home = atlas_link::atlas_home();
    let port = atlas_link::configured_port(&home);
    if let Err(error) = atlas_link::stop(port, &home) {
        eprintln!(
            "[atlas] {}",
            tr!(
                "arrêt du daemon impossible : {error}",
                "the daemon could not be stopped: {error}"
            )
        );
    }
    if atlas_link::is_running(port) && !daemon::wait_until_stopped(port, STOP_TIMEOUT) {
        eprintln!(
            "[atlas] {}",
            tr!(
                "le daemon répond encore sur le port {port} : l'interface reste ouverte",
                "the daemon still answers on port {port}: the interface stays open"
            )
        );
        show_factory(app);
        return;
    }
    app.exit(0);
}

/// The items of the tray menu, kept so they can be worded again when the language changes.
struct TrayLabels {
    open: MenuItem<tauri::Wry>,
    quit_shell: MenuItem<tauri::Wry>,
    stop_daemon: MenuItem<tauri::Wry>,
}

fn open_label() -> String {
    tr!("Ouvrir Atlas", "Open Atlas")
}

fn quit_shell_label() -> String {
    tr!(
        "Quitter l'interface (les agents continuent)",
        "Quit the interface (the agents carry on)"
    )
}

fn stop_daemon_label() -> String {
    tr!("Arrêter Atlas et ses agents", "Stop Atlas and its agents")
}

/// Called by the page with the language the factory speaks, at start and each time the master
/// changes it: the shell speaks the system's until then.
#[tauri::command]
fn set_language(app: AppHandle, language: Language) {
    atlas_i18n::set_current(language);
    // No tray on a machine that lacks its library: nothing to word again.
    if let Some(labels) = app.try_state::<TrayLabels>() {
        let _ = labels.open.set_text(open_label());
        let _ = labels.quit_shell.set_text(quit_shell_label());
        let _ = labels.stop_daemon.set_text(stop_daemon_label());
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, open_label(), true, None::<&str>)?;
    let quit_shell = MenuItem::with_id(app, MENU_QUIT_SHELL, quit_shell_label(), true, None::<&str>)?;
    let stop_daemon = MenuItem::with_id(app, MENU_STOP_DAEMON, stop_daemon_label(), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &PredefinedMenuItem::separator(app)?, &quit_shell, &stop_daemon],
    )?;
    let mut tray = TrayIconBuilder::with_id("atlas")
        .tooltip("Atlas")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => show_factory(app),
            MENU_QUIT_SHELL => app.exit(0),
            MENU_STOP_DAEMON => stop_daemon_and_quit(app),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayLabels {
        open,
        quit_shell,
        stop_daemon,
    });
    Ok(())
}

fn main() {
    // The language of the system until the page tells which one the factory speaks.
    atlas_i18n::set_current(Language::system());
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_factory(app)))
        .manage(Startup(Mutex::new(())))
        .invoke_handler(tauri::generate_handler![connect_to_daemon, set_language])
        .setup(|app| {
            let window = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
                .title("Atlas")
                .inner_size(1440.0, 900.0)
                .min_inner_size(960.0, 600.0)
                // A link of the page (a merge request, a link in a report) opens in the browser of
                // the machine: the shell has no second window, and without this the click is lost.
                .on_new_window(|url, _features| {
                    if matches!(url.scheme(), "http" | "https")
                        && let Err(error) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
                    {
                        eprintln!(
                            "[atlas] {}",
                            tr!(
                                "ouverture de {url} impossible : {error}",
                                "{url} could not be opened: {error}"
                            )
                        );
                    }
                    NewWindowResponse::Deny
                })
                .build()?;
            let hidden = window.clone();
            window.on_window_event(move |event| {
                // The window is a view on the factory: closing it hides it, the tray brings it back.
                // Not on Linux: stock GNOME shows no tray, and a hidden window would be out of reach.
                // The shell quits there, the daemon carries on, and the next launch finds it.
                if let WindowEvent::CloseRequested { api, .. } = event
                    && cfg!(not(target_os = "linux"))
                {
                    api.prevent_close();
                    let _ = hidden.hide();
                }
            });
            let handle = app.handle().clone();
            // The tray library of Linux is loaded when the icon is built and panics when the machine
            // lacks it: the factory opens without its tray rather than not at all.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_tray(&handle))) {
                Ok(built) => built?,
                Err(_) => eprintln!(
                    "[atlas] {}",
                    tr!(
                        "icône de la barre indisponible sur cette machine",
                        "tray icon unavailable on this machine"
                    )
                ),
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Atlas could not start");

    app.run(|_app, _event| {
        // A click on the Dock icon while the window is hidden.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = _event {
            show_factory(_app);
        }
    });
}
