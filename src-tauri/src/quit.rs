//! Quitting (spec §4.4): one confirmation listing busy sessions and running
//! creates, then SIGHUP every PTY and SIGINT every create.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Runtime};

use crate::app::App;

/// Set once the user confirmed; later exit and close requests go through.
static QUITTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuitInfo {
    pub busy: Vec<BusyProcess>,
    /// Ids of planes still being created.
    pub creating: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BusyProcess {
    pub plane_id: String,
    pub process_name: String,
}

impl QuitInfo {
    pub fn needs_confirmation(&self) -> bool {
        !self.busy.is_empty() || !self.creating.is_empty()
    }
}

pub fn check(app: &App) -> QuitInfo {
    QuitInfo {
        busy: app
            .sessions
            .busy_sessions()
            .into_iter()
            .map(|(plane_id, process_name)| BusyProcess {
                plane_id,
                process_name,
            })
            .collect(),
        creating: app.creates.running_ids(),
    }
}

pub fn quit<R: Runtime>(handle: &AppHandle<R>, app: &App) {
    QUITTING.store(true, Ordering::SeqCst);
    app.sessions.hang_up_all();
    app.creates.interrupt_all();
    handle.exit(0);
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}
