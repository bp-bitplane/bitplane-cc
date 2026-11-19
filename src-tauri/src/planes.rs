//! The sidebar's planes and drafts, mirrored from the daemon's plane module
//! (sessiond ADR 0033): `plane.list` on every connection, then every
//! `plane.changed`; refresh, projects and drafts go through it too.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bitplane_protocol::hello::Capability;
use bitplane_protocol::plane::{
    PlaneCreateDraftParams, PlaneProjectsResult, PlanesModel as DaemonModel,
};
use bitplane_protocol::{ErrorCode, Frame, Notification, method};
use bitplane_sessiond::client::Handle;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tokio::sync::watch;

use crate::contract::Envelope;
use crate::daemon::{self, DaemonLink, Subscriber};

/// The daemon's module name for planes in `hello`.
pub const MODULE: &str = "plane";

/// One sidebar row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaneRow {
    pub id: String,
    pub directory: String,
    pub kind: PlaneKind,
    /// Finding tags from `bp list`; empty unless `kind` is `Plane`.
    pub findings: Vec<String>,
    /// Member specs from `bp list`; empty unless `kind` is `Plane`.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PlaneKind {
    Plane,
    Draft,
    Creating,
    Gone,
}

/// Whether the plane list can be shown, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AppStatus {
    Loading,
    Ready,
    /// The daemon's plane module is off; `reason` names the fix.
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
    },
    /// git missing or older than 2.36.
    #[serde(rename_all = "camelCase")]
    GitMissing {
        message: String,
    },
    /// `bp list` failed for another reason.
    #[serde(rename_all = "camelCase")]
    ListFailed {
        message: String,
    },
}

/// A plane being created, which owns its row until it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Creating {
    pub id: String,
    pub directory: Option<String>,
}

pub struct Planes {
    me: Weak<Planes>,
    on_change: Arc<dyn Fn() + Send + Sync>,
    state: Mutex<State>,
    ready: watch::Sender<bool>,
}

#[derive(Default)]
struct State {
    handle: Option<Handle>,
    model: Option<DaemonModel>,
    connection: u64,
    /// Bumped per `plane.changed`; a list reply older than one is dropped.
    changes: u64,
}

impl Planes {
    /// Follows `link` from its next connection on; `on_change` runs after
    /// every new model.
    pub fn new(link: &DaemonLink, on_change: Arc<dyn Fn() + Send + Sync>) -> Arc<Self> {
        let planes = Arc::new_cyclic(|me| Planes {
            me: me.clone(),
            on_change,
            state: Mutex::default(),
            ready: watch::Sender::new(false),
        });
        link.subscribe(Arc::new(Subscription(Arc::downgrade(&planes))));
        planes
    }

    /// Resolves once the first `plane.list` answered, or after `timeout`.
    pub async fn ready(&self, timeout: Duration) {
        let mut ready = self.ready.subscribe();
        let _ = tokio::time::timeout(timeout, ready.wait_for(|ready| *ready)).await;
    }

    /// The daemon's latest model, gone rows and all.
    pub fn model(&self) -> Option<DaemonModel> {
        self.state.lock().unwrap().model.clone()
    }

    /// Re-read now; the answer arrives as `plane.changed`.
    pub async fn refresh(&self) -> Result<(), String> {
        self.call::<Value>(method::PLANE_REFRESH, json!({}))
            .await
            .map(drop)
            .map_err(|error| error.message)
    }

    pub async fn projects(&self) -> Result<Vec<Project>, String> {
        let result: PlaneProjectsResult = self
            .call(method::PLANE_PROJECTS, json!({}))
            .await
            .map_err(|error| error.message)?;
        Ok(result
            .projects
            .into_iter()
            .map(|project| Project {
                name: project.name,
                source_repo_present: project.source_repo_present,
            })
            .collect())
    }

    /// An empty folder in the planes directory; the row follows as `plane.changed`.
    pub async fn create_draft(&self, id: &str) -> Result<(), String> {
        let params = PlaneCreateDraftParams { id: id.into() };
        match self.call::<Value>(method::PLANE_CREATE_DRAFT, params).await {
            Ok(_) => Ok(()),
            Err(error) if error.code == Some(ErrorCode::PlaneExists) => {
                Err(format!("{id} already exists"))
            }
            Err(error) => Err(error.message),
        }
    }

    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, daemon::CallError> {
        let handle = self.state.lock().unwrap().handle.clone();
        daemon::call(handle.as_ref(), method, params).await
    }

    fn connected(&self, handle: Handle) {
        let (connection, changes) = {
            let mut state = self.state.lock().unwrap();
            state.handle = Some(handle.clone());
            state.connection += 1;
            (state.connection, state.changes)
        };
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let listed = daemon::call::<DaemonModel>(Some(&handle), method::PLANE_LIST, json!({}))
                .await
                .ok();
            let Some(planes) = me.upgrade() else { return };
            {
                let mut state = planes.state.lock().unwrap();
                if state.connection != connection {
                    return;
                }
                if let Some(model) = listed.filter(|_| state.changes == changes) {
                    state.model = Some(model);
                }
            }
            planes.ready.send_replace(true);
            (planes.on_change)();
        });
    }

    fn event(&self, event: Notification) {
        if event.method != method::PLANE_CHANGED {
            return;
        }
        let Some(model) = event
            .params
            .and_then(|params| serde_json::from_value::<DaemonModel>(params).ok())
        else {
            return;
        };
        {
            let mut state = self.state.lock().unwrap();
            state.changes += 1;
            state.model = Some(model);
        }
        self.ready.send_replace(true);
        (self.on_change)();
    }
}

/// One project from `plane.projects`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub source_repo_present: bool,
}

/// The sidebar's rows: the daemon's, with a plane being created in place of
/// what `bp list` says about it (its findings are noise until it ends), and
/// gone rows only while one of cc's sessions is still in them: the daemon
/// keeps them until it restarts.
pub fn rows(
    model: &DaemonModel,
    creating: &[Creating],
    with_sessions: &BTreeSet<String>,
) -> Vec<PlaneRow> {
    use bitplane_protocol::plane::PlaneKind as Kind;
    let mut rows: Vec<PlaneRow> = model
        .planes
        .iter()
        .filter(|row| !creating.iter().any(|c| c.id == row.id))
        .filter_map(|row| {
            let kind = match row.kind {
                Kind::Plane => PlaneKind::Plane,
                Kind::Draft => PlaneKind::Draft,
                Kind::Gone if with_sessions.contains(&row.id) => PlaneKind::Gone,
                Kind::Gone | Kind::Unknown => return None,
            };
            Some(PlaneRow {
                id: row.id.clone(),
                directory: row.directory.clone(),
                kind,
                findings: row.findings.clone(),
                members: row.members.iter().map(|m| m.member.clone()).collect(),
            })
        })
        .collect();
    for create in creating {
        if rows.iter().any(|row| row.id == create.id) {
            continue;
        }
        let directory = create.directory.clone().unwrap_or_else(|| {
            PathBuf::from(&model.planes_dir)
                .join(&create.id)
                .to_string_lossy()
                .into_owned()
        });
        rows.push(PlaneRow {
            id: create.id.clone(),
            directory,
            kind: PlaneKind::Creating,
            findings: Vec::new(),
            members: Vec::new(),
        });
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

/// What the main area shows instead of planes, from the plane capability in
/// `hello` and the last `bp list`.
pub fn status(capability: Option<&Capability>, model: Option<&DaemonModel>) -> AppStatus {
    if let Some(capability) = capability.filter(|c| !c.available) {
        return AppStatus::Unavailable {
            reason: capability
                .reason
                .clone()
                .unwrap_or_else(|| "the daemon gave no reason".into()),
        };
    }
    let Some(model) = model.filter(|m| m.refreshed_at.is_some() || m.list_error.is_some()) else {
        return AppStatus::Loading;
    };
    match &model.list_error {
        None => AppStatus::Ready,
        Some(error) => {
            let envelope = error
                .envelope
                .clone()
                .and_then(|e| serde_json::from_value::<Envelope>(e).ok());
            match envelope {
                Some(envelope) if envelope.error == "git_missing" => AppStatus::GitMissing {
                    message: envelope.message,
                },
                _ => AppStatus::ListFailed {
                    message: error.message.clone(),
                },
            }
        }
    }
}

/// What `Planes` subscribes with; weak, so the link doesn't keep it alive.
struct Subscription(Weak<Planes>);

impl Subscriber for Subscription {
    fn connected(&self, handle: Handle) {
        if let Some(planes) = self.0.upgrade() {
            planes.connected(handle);
        }
    }

    fn event(&self, event: Notification) {
        if let Some(planes) = self.0.upgrade() {
            planes.event(event);
        }
    }

    fn frame(&self, _frame: Frame) {}
}

#[cfg(test)]
mod tests {
    use bitplane_protocol::plane::{
        PlaneKind as Kind, PlaneListError, PlaneMember, PlaneRow as Row,
    };

    use super::*;

    fn row(id: &str, kind: Kind) -> Row {
        Row {
            id: id.into(),
            directory: format!("/p/{id}"),
            kind,
            members: vec![PlaneMember {
                member: "@api".into(),
                path: format!("/p/{id}/api"),
            }],
            findings: vec!["create_never_completed".into()],
        }
    }

    fn model(rows: Vec<Row>) -> DaemonModel {
        DaemonModel {
            planes_dir: "/p".into(),
            planes_dir_exists: true,
            refreshed_at: Some(1),
            list_error: None,
            planes: rows,
        }
    }

    #[test]
    fn gone_rows_show_only_while_cc_has_sessions_in_them() {
        let model = model(vec![
            row("alpha", Kind::Draft),
            row("beta", Kind::Plane),
            row("new", Kind::Plane),
            row("old", Kind::Gone),
            row("older", Kind::Gone),
            row("odd", Kind::Unknown),
        ]);
        let creating = [
            Creating {
                id: "new".into(),
                directory: None,
            },
            Creating {
                id: "next".into(),
                directory: Some("/elsewhere/next".into()),
            },
        ];
        let rows = rows(&model, &creating, &BTreeSet::from(["old".to_string()]));
        let summary: Vec<_> = rows
            .iter()
            .map(|r| (r.id.as_str(), r.kind, r.findings.len(), r.members.len()))
            .collect();
        assert_eq!(
            summary,
            [
                ("alpha", PlaneKind::Draft, 1, 1),
                ("beta", PlaneKind::Plane, 1, 1),
                ("new", PlaneKind::Creating, 0, 0),
                ("next", PlaneKind::Creating, 0, 0),
                ("old", PlaneKind::Gone, 1, 1),
            ]
        );
        assert_eq!(rows[2].directory, "/p/new");
        assert_eq!(rows[3].directory, "/elsewhere/next");
        assert_eq!(rows[1].members, ["@api"]);
    }

    #[test]
    fn status_follows_the_capability_and_the_last_list() {
        let off = Capability {
            module: MODULE.into(),
            available: false,
            reason: Some("bp not found on PATH".into()),
            info: None,
        };
        assert_eq!(
            status(Some(&off), None),
            AppStatus::Unavailable {
                reason: "bp not found on PATH".into()
            }
        );
        assert_eq!(status(None, None), AppStatus::Loading);
        let mut listed = model(vec![]);
        listed.refreshed_at = None;
        assert_eq!(status(None, Some(&listed)), AppStatus::Loading);
        listed.refreshed_at = Some(1);
        assert_eq!(status(None, Some(&listed)), AppStatus::Ready);

        listed.list_error = Some(PlaneListError {
            message: "git is too old".into(),
            envelope: Some(serde_json::json!({
                "error": "git_missing", "code": 4, "message": "git is too old",
                "problems": [], "remedy": null
            })),
        });
        assert_eq!(
            status(None, Some(&listed)),
            AppStatus::GitMissing {
                message: "git is too old".into()
            }
        );
        listed.list_error = Some(PlaneListError {
            message: "bp could not be run".into(),
            envelope: None,
        });
        assert_eq!(
            status(None, Some(&listed)),
            AppStatus::ListFailed {
                message: "bp could not be run".into()
            }
        );
    }
}
