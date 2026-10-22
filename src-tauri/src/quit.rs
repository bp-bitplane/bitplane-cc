//! Quitting (spec §4.4, ADR 0005): one confirmation listing busy sessions and
//! running creates, unless every busy session resumes on the next launch; then
//! save the sessions, SIGHUP every PTY and SIGINT every create.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Runtime};

use crate::app::App;

/// Set once the user confirmed; later exit and close requests go through.
static QUITTING: AtomicBool = AtomicBool::new(false);
static TORN_DOWN: AtomicBool = AtomicBool::new(false);

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
    /// An agent session runs here and restore will resume it.
    pub resumes: bool,
    /// The agent is mid-turn; resuming won't finish the turn.
    pub working: bool,
}

impl QuitInfo {
    pub fn needs_confirmation(&self) -> bool {
        self.busy.iter().any(|b| !b.resumes || b.working) || !self.creating.is_empty()
    }
}

pub fn check(app: &App) -> QuitInfo {
    QuitInfo {
        busy: app
            .sessions
            .busy_sessions()
            .into_iter()
            .map(|busy| {
                // A link left by a claude that crashed says nothing about the
                // vim running there now.
                let link = app
                    .agent_link(&busy.key)
                    .filter(|_| busy.process_name.starts_with("claude"))
                    .filter(|l| l.resumable_in(&busy.plane_dir));
                BusyProcess {
                    plane_id: busy.plane_id,
                    process_name: busy.process_name,
                    resumes: link.is_some(),
                    working: link.is_some_and(|l| l.working),
                }
            })
            .collect(),
        creating: app.creates.running_ids(),
    }
}

pub fn quit<R: Runtime>(handle: &AppHandle<R>, app: &App) {
    QUITTING.store(true, Ordering::SeqCst);
    tear_down(app);
    handle.exit(0);
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// Runs once, from `quit` or from an exit that skipped it (a Dock Quit), so
/// a create isn't sent a second SIGINT that would cut its cleanup short.
pub fn tear_down(app: &App) {
    if TORN_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    app.prepare_quit();
    app.sessions.hang_up_all();
    app.creates.interrupt_all();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy(resumes: bool, working: bool) -> BusyProcess {
        BusyProcess {
            plane_id: "alpha".into(),
            process_name: "claude".into(),
            resumes,
            working,
        }
    }

    #[test]
    fn confirms_only_when_something_would_be_lost() {
        let info = |busy: Vec<BusyProcess>, creating: Vec<String>| QuitInfo { busy, creating };
        assert!(!info(vec![], vec![]).needs_confirmation());
        assert!(!info(vec![busy(true, false)], vec![]).needs_confirmation());
        assert!(info(vec![busy(true, true)], vec![]).needs_confirmation());
        assert!(info(vec![busy(false, false)], vec![]).needs_confirmation());
        assert!(info(vec![], vec!["beta".into()]).needs_confirmation());
    }
}
