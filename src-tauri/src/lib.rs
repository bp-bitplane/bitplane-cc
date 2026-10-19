//! bitplane Command Center: a sidebar of bitplane planes and tabbed terminal
//! sessions rooted in them.

pub mod agents;
pub mod app;
pub mod bp;
pub mod create;
pub mod diff;
pub mod editor;
pub mod env;
pub mod files;
pub mod files_watch;
pub mod git;
pub mod git_cli;
pub mod git_watch;
pub mod hooks;
pub mod ipc;
pub mod persist;
pub mod planes;
pub mod quit;
pub mod sessions;

use std::sync::Arc;

use tauri::menu::{Menu, PredefinedMenuItem, Submenu};
use tauri::{Manager, RunEvent, Runtime, WindowEvent};
use tauri_plugin_window_state::StateFlags;
use tauri_specta::Event;

use crate::app::{App, DataDir, Emitters};
use crate::ipc::{
    CreateChanged, FileTabsChanged, FilesChanged, PlanesChanged, QuitRequested, SessionChanged,
    SourceControlChanged,
};

/// Where tauri-specta writes the TypeScript bindings.
pub const BINDINGS_PATH: &str = "../src/ipc/bindings.ts";

pub fn run() {
    let builder = ipc::builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder, BINDINGS_PATH).expect("exporting TypeScript bindings");
    let invoke_handler = ipc::invoke_handler(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                .build(),
        )
        .menu(menu)
        .invoke_handler(invoke_handler)
        .setup(move |tauri_app| {
            builder.mount_events(tauri_app);
            let handle = tauri_app.handle().clone();
            let emitters = Emitters {
                planes: {
                    let handle = handle.clone();
                    Arc::new(move |model| {
                        let _ = PlanesChanged(model).emit(&handle);
                    })
                },
                session: {
                    let handle = handle.clone();
                    Arc::new(move |event| {
                        let _ = SessionChanged(event).emit(&handle);
                    })
                },
                create: {
                    let handle = handle.clone();
                    Arc::new(move |event| {
                        let _ = CreateChanged(event).emit(&handle);
                    })
                },
                source_control: {
                    let handle = handle.clone();
                    Arc::new(move |event| {
                        let _ = SourceControlChanged(event).emit(&handle);
                    })
                },
                file_tabs: {
                    let handle = handle.clone();
                    Arc::new(move |event| {
                        let _ = FileTabsChanged(event).emit(&handle);
                    })
                },
                files: {
                    let handle = handle.clone();
                    Arc::new(move |event| {
                        let _ = FilesChanged(event).emit(&handle);
                    })
                },
            };
            let data_dir = match tauri_app.path().app_data_dir() {
                Ok(dir) => Some(DataDir(dir)),
                Err(error) => {
                    eprintln!("bitplane-cc: no app data directory, nothing is restored: {error}");
                    None
                }
            };
            let app = App::new(
                env::original(),
                tauri_app.package_info().version.to_string(),
                data_dir,
                emitters,
            );
            app.restore();
            tauri_app.manage::<ipc::AppState>(Arc::clone(&app));
            tauri::async_runtime::spawn(async move {
                app.start();
                app.refresh().await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && !quit::is_quitting()
            {
                api.prevent_close();
                let _ = QuitRequested.emit(window.app_handle());
            }
        })
        .build(tauri::generate_context!())
        .expect("building the tauri application")
        .run(|handle, event| {
            // ⌘Q arrives without a code; `quit` exits with one.
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = event
                && !quit::is_quitting()
            {
                api.prevent_exit();
                let _ = QuitRequested.emit(handle);
            }
        });
}

/// Write the TypeScript bindings to `path`.
pub fn export_bindings<R: Runtime>(
    builder: &tauri_specta::Builder<R>,
    path: impl AsRef<std::path::Path>,
) -> Result<(), String> {
    builder
        .export(
            specta_typescript::Typescript::default().header("// @ts-nocheck"),
            path,
        )
        .map_err(|e| e.to_string())
}

/// The standard menus minus Close Window, whose ⌘W closes a session here.
fn menu<R: Runtime>(handle: &tauri::AppHandle<R>) -> tauri::Result<Menu<R>> {
    let name = handle.package_info().name.clone();
    Menu::with_items(
        handle,
        &[
            &Submenu::with_items(
                handle,
                &name,
                true,
                &[
                    &PredefinedMenuItem::about(handle, None, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::services(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::hide(handle, None)?,
                    &PredefinedMenuItem::hide_others(handle, None)?,
                    &PredefinedMenuItem::show_all(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::quit(handle, None)?,
                ],
            )?,
            &Submenu::with_items(
                handle,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(handle, None)?,
                    &PredefinedMenuItem::redo(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::cut(handle, None)?,
                    &PredefinedMenuItem::copy(handle, None)?,
                    &PredefinedMenuItem::paste(handle, None)?,
                    &PredefinedMenuItem::select_all(handle, None)?,
                ],
            )?,
            &Submenu::with_items(
                handle,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(handle, None)?,
                    &PredefinedMenuItem::maximize(handle, None)?,
                    &PredefinedMenuItem::fullscreen(handle, None)?,
                ],
            )?,
        ],
    )
}
