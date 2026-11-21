//! Plane creation as daemon jobs (sessiond ADR 0034): `plane.create` starts
//! one, and every job of that kind is mirrored from `job.list` and the job
//! events, its log through `job.watch`. A create therefore outlives cc, and
//! a relaunch shows it with its log tail and the client that started it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bitplane_protocol::job::{
    Job, JobChanged, JobKind, JobListResult, JobLog, JobPhase, JobRef, JobStatus, JobWatchParams,
};
use bitplane_protocol::plane::{PlaneCreateDetail, PlaneCreateParams, PlaneCreateResult};
use bitplane_protocol::{ErrorCode, Frame, Notification, method};
use bitplane_sessiond::client::Handle;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tokio::sync::watch;

use crate::contract::{BranchIntent, Envelope};
use crate::daemon::{self, DaemonLink, Subscriber};
use crate::planes::Creating;

/// What the create dialog asks for; mapped onto bp's `PlaneCreateRequest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateArgs {
    pub id: String,
    pub branch: String,
    pub members: Vec<Member>,
    pub intent: BranchIntent,
    pub no_fetch: bool,
    pub no_scripts: bool,
}

/// `@project` or a repo path, with an optional branch override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Member {
    pub spec: String,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Fetching,
    AddingWorktrees,
    RunningScripts,
}

/// bp's answer, never what the disk looks like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CreateStatus {
    Running,
    Succeeded,
    /// The plane was built but a post-create script failed; treated as a success.
    ScriptFailed,
    /// Nothing is on disk; the dialog reopens.
    #[serde(rename_all = "camelCase")]
    NothingCreated {
        envelope: Option<Envelope>,
        retryable: bool,
    },
    /// A partial plane is left behind; only `bp destroy` cleans it up.
    #[serde(rename_all = "camelCase")]
    Partial {
        envelope: Option<Envelope>,
    },
    /// No answer from bp: it couldn't be run, died, or the daemon restarted
    /// under it, which may have left a partial plane.
    #[serde(rename_all = "camelCase")]
    Failed {
        reason: String,
    },
}

/// One create, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateInfo {
    pub job: String,
    /// The plane's id.
    pub id: String,
    /// What this launch of cc asked for; unknown for a create started
    /// elsewhere or before a relaunch.
    pub args: Option<CreateArgs>,
    pub phase: Option<Phase>,
    pub status: CreateStatus,
    /// bp's stderr, as much of it as the daemon kept.
    pub log: String,
    /// The client that started it, with its version.
    pub started_by: String,
    /// Unix time in milliseconds.
    pub started_at: u64,
}

/// `create_changed`: one create, or `None` once its job is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CreateEvent {
    pub job: String,
    pub id: String,
    pub create: Option<CreateInfo>,
}

/// `create_log`: lines a running create appended to its log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CreateLogEvent {
    pub job: String,
    pub id: String,
    pub text: String,
}

/// Where job changes go.
pub struct JobEmitters {
    pub create: Arc<dyn Fn(CreateEvent) + Send + Sync>,
    pub log: Arc<dyn Fn(CreateLogEvent) + Send + Sync>,
    /// After any create starts, ends or goes: the sidebar's rows follow.
    pub changed: Arc<dyn Fn() + Send + Sync>,
}

pub struct Jobs {
    me: Weak<Jobs>,
    emit: JobEmitters,
    state: Mutex<State>,
    /// Held while a `job.watch` is out, so scopes reach the daemon in order.
    sending: tokio::sync::Mutex<()>,
    ready: watch::Sender<bool>,
}

#[derive(Default)]
struct State {
    handle: Option<Handle>,
    connection: u64,
    jobs: BTreeMap<String, Entry>,
    args: HashMap<String, CreateArgs>,
    /// While a `job.list` is in flight: what the events said meanwhile,
    /// which is newer than the list.
    listing: Option<Touched>,
    /// The log scope the daemon holds; `None` on a fresh connection.
    sent: Option<BTreeSet<String>>,
    /// Jobs whose next `job.log` is their backlog, which replaces the log.
    backlog_due: HashSet<String>,
}

#[derive(Default)]
struct Touched {
    changed: HashSet<String>,
    removed: HashSet<String>,
}

struct Entry {
    job: Job,
    plane: String,
    lines: Vec<String>,
    dropped: u64,
}

impl Jobs {
    /// Follows `link` from its next connection on.
    pub fn new(link: &DaemonLink, emit: JobEmitters) -> Arc<Self> {
        let jobs = Arc::new_cyclic(|me| Jobs {
            me: me.clone(),
            emit,
            state: Mutex::default(),
            sending: tokio::sync::Mutex::new(()),
            ready: watch::Sender::new(false),
        });
        link.subscribe(Arc::new(Subscription(Arc::downgrade(&jobs))));
        jobs
    }

    /// Resolves once the first `job.list` is mirrored, or after `timeout`.
    pub async fn ready(&self, timeout: Duration) {
        let mut ready = self.ready.subscribe();
        let _ = tokio::time::timeout(timeout, ready.wait_for(|ready| *ready)).await;
    }

    pub fn list(&self) -> Vec<CreateInfo> {
        let state = self.state.lock().unwrap();
        state
            .jobs
            .values()
            .map(|entry| info(entry, state.args.get(&entry.job.id)))
            .collect()
    }

    /// Creates that own a sidebar row: running, or failed partway.
    pub fn creating(&self) -> Vec<Creating> {
        let state = self.state.lock().unwrap();
        state
            .jobs
            .values()
            .filter(|entry| matches!(entry.job.status, JobStatus::Running | JobStatus::Partial))
            .map(|entry| Creating {
                id: entry.plane.clone(),
                directory: detail(&entry.job).directory,
            })
            .collect()
    }

    /// Start `bp create`; returns the job. A finished create of the same
    /// plane is dismissed, so the plane shows only the new one.
    pub async fn create(&self, args: CreateArgs) -> Result<String, String> {
        let params = PlaneCreateParams {
            args: request(&args),
        };
        let PlaneCreateResult { job } = self
            .call(method::PLANE_CREATE, params)
            .await
            .map_err(|error| error.message)?;
        let (event, stale) = {
            let mut state = self.state.lock().unwrap();
            let stale: Vec<String> = state
                .jobs
                .values()
                .filter(|entry| entry.plane == args.id && !entry.job.status.is_running())
                .map(|entry| entry.job.id.clone())
                .collect();
            state.args.insert(job.clone(), args);
            let event = state.jobs.get(&job).map(|entry| CreateEvent {
                job: job.clone(),
                id: entry.plane.clone(),
                create: Some(info(entry, state.args.get(&job))),
            });
            (event, stale)
        };
        if let Some(event) = event {
            (self.emit.create)(event);
        }
        for old in stale {
            let _ = self.dismiss(&old).await;
        }
        Ok(job)
    }

    /// SIGINT to bp; its answer still decides the outcome.
    pub async fn cancel(&self, job: &str) -> Result<(), String> {
        let params = JobRef { job: job.into() };
        match self.call::<Value>(method::JOB_CANCEL, params).await {
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.code,
                    Some(ErrorCode::UnknownJob | ErrorCode::JobFinished)
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(error.message),
        }
    }

    /// Forget a finished create, for every client.
    pub async fn dismiss(&self, job: &str) -> Result<(), String> {
        let params = JobRef { job: job.into() };
        match self.call::<Value>(method::JOB_DISMISS, params).await {
            Ok(_) => Ok(()),
            Err(error) if error.code == Some(ErrorCode::UnknownJob) => {
                self.removed(job);
                Ok(())
            }
            Err(error) => Err(error.message),
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, daemon::CallError> {
        let handle = self.state.lock().unwrap().handle.clone();
        daemon::call(handle.as_ref(), method, params).await
    }

    fn connected(&self, handle: Handle) {
        let connection = {
            let mut state = self.state.lock().unwrap();
            state.handle = Some(handle.clone());
            state.connection += 1;
            state.listing = Some(Touched::default());
            state.sent = None;
            state.backlog_due.clear();
            state.connection
        };
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let listed =
                daemon::call::<JobListResult>(Some(&handle), method::JOB_LIST, json!({})).await;
            let Some(jobs) = me.upgrade() else { return };
            jobs.resync(connection, listed.ok());
        });
    }

    /// Mirror `job.list`, letting what the events said meanwhile win.
    fn resync(&self, connection: u64, listed: Option<JobListResult>) {
        let events = {
            let mut state = self.state.lock().unwrap();
            if state.connection != connection {
                return;
            }
            let touched = state.listing.take().unwrap_or_default();
            let mut events = Vec::new();
            if let Some(listed) = listed {
                let ids: HashSet<String> = listed.jobs.iter().map(|job| job.id.clone()).collect();
                for job in listed.jobs {
                    if touched.changed.contains(&job.id) || touched.removed.contains(&job.id) {
                        continue;
                    }
                    events.extend(upsert(&mut state, job));
                }
                let vanished: Vec<String> = state
                    .jobs
                    .keys()
                    .filter(|id| !ids.contains(*id) && !touched.changed.contains(*id))
                    .cloned()
                    .collect();
                for id in vanished {
                    events.extend(remove(&mut state, &id));
                }
            }
            events
        };
        self.ready.send_replace(true);
        for event in events {
            (self.emit.create)(event);
        }
        (self.emit.changed)();
        self.sync_watch();
    }

    fn event(&self, event: Notification) {
        match event.method.as_str() {
            method::JOB_CHANGED => {
                let Some(JobChanged { job }) = params(event.params) else {
                    return;
                };
                if job.kind != JobKind::PlaneCreate {
                    return;
                }
                let emitted = {
                    let mut state = self.state.lock().unwrap();
                    if let Some(touched) = state.listing.as_mut() {
                        touched.changed.insert(job.id.clone());
                    }
                    upsert(&mut state, job)
                };
                if let Some(event) = emitted {
                    (self.emit.create)(event);
                    (self.emit.changed)();
                    self.sync_watch();
                }
            }
            method::JOB_REMOVED => {
                let Some(JobRef { job }) = params(event.params) else {
                    return;
                };
                if let Some(touched) = self.state.lock().unwrap().listing.as_mut() {
                    touched.removed.insert(job.clone());
                }
                self.removed(&job);
            }
            method::JOB_LOG => {
                if let Some(log) = params::<JobLog>(event.params) {
                    self.log(log);
                }
            }
            _ => {}
        }
    }

    fn log(&self, log: JobLog) {
        enum Emit {
            Whole(Box<CreateEvent>),
            Lines(CreateLogEvent),
        }
        let emit = {
            let mut state = self.state.lock().unwrap();
            let backlog = state.backlog_due.remove(&log.job);
            let args = state.args.get(&log.job).cloned();
            let Some(entry) = state.jobs.get_mut(&log.job) else {
                return;
            };
            if backlog {
                entry.lines = log.lines;
                entry.dropped = log.dropped.unwrap_or(0);
                Emit::Whole(Box::new(CreateEvent {
                    job: log.job,
                    id: entry.plane.clone(),
                    create: Some(info(entry, args.as_ref())),
                }))
            } else {
                let text = log.lines.join("\n");
                entry.lines.extend(log.lines);
                Emit::Lines(CreateLogEvent {
                    job: log.job,
                    id: entry.plane.clone(),
                    text,
                })
            }
        };
        match emit {
            Emit::Whole(event) => (self.emit.create)(*event),
            Emit::Lines(event) => (self.emit.log)(event),
        }
    }

    fn removed(&self, job: &str) {
        let event = {
            let mut state = self.state.lock().unwrap();
            state.args.remove(job);
            remove(&mut state, job)
        };
        if let Some(event) = event {
            (self.emit.create)(event);
            (self.emit.changed)();
            self.sync_watch();
        }
    }

    /// Watch every mirrored job's log; the latest scope wins, as it's read
    /// once the previous call is done.
    fn sync_watch(&self) {
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let Some(jobs) = me.upgrade() else { return };
            let _sending = jobs.sending.lock().await;
            let (handle, wanted) = {
                let mut state = jobs.state.lock().unwrap();
                if state.listing.is_some() {
                    return;
                }
                let wanted: BTreeSet<String> = state.jobs.keys().cloned().collect();
                if state.sent.as_ref() == Some(&wanted) {
                    return;
                }
                let added: Vec<String> = match &state.sent {
                    Some(sent) => wanted.difference(sent).cloned().collect(),
                    None => wanted.iter().cloned().collect(),
                };
                state.backlog_due.extend(added);
                (state.handle.clone(), wanted)
            };
            let params = JobWatchParams {
                jobs: wanted.iter().cloned().collect(),
            };
            if daemon::call::<Value>(handle.as_ref(), method::JOB_WATCH, params)
                .await
                .is_ok()
            {
                jobs.state.lock().unwrap().sent = Some(wanted);
            }
        });
    }
}

/// bp's `PlaneCreateRequest` for what the dialog asked.
pub fn request(args: &CreateArgs) -> Value {
    let members: Vec<String> = args
        .members
        .iter()
        .map(
            |member| match member.branch.as_deref().filter(|b| !b.is_empty()) {
                Some(branch) => format!("{}:{branch}", member.spec),
                None => member.spec.clone(),
            },
        )
        .collect();
    json!({
        "members": members,
        "branch": args.branch,
        "id": args.id,
        "here": null,
        "intent": args.intent,
        // bp refuses a resolve without a fetch, so the dialog only offers
        // skipping it with New or Existing.
        "fetch": !(args.no_fetch && args.intent != BranchIntent::Resolve),
        "run_scripts": !args.no_scripts,
    })
}

/// A create's status from its job: the state is bp's answer as the daemon
/// mapped it, and the envelope is bp's own.
pub fn status(job: &Job) -> CreateStatus {
    let envelope = detail(job)
        .envelope
        .and_then(|envelope| serde_json::from_value::<Envelope>(envelope).ok());
    match &job.status {
        JobStatus::Running => CreateStatus::Running,
        JobStatus::Succeeded => CreateStatus::Succeeded,
        JobStatus::ScriptFailed => CreateStatus::ScriptFailed,
        JobStatus::NothingCreated => CreateStatus::NothingCreated {
            retryable: envelope.as_ref().is_some_and(|e| e.error == "lock_timeout"),
            envelope,
        },
        JobStatus::Partial => CreateStatus::Partial { envelope },
        JobStatus::Failed { reason } => CreateStatus::Failed {
            reason: reason.clone(),
        },
        JobStatus::Unknown => CreateStatus::Failed {
            reason: "the daemon reports a state this cc doesn't know".into(),
        },
    }
}

/// The log as one text, saying how much the daemon's cap evicted.
pub fn log_text(lines: &[String], dropped: u64) -> String {
    let text = lines.join("\n");
    match dropped {
        0 => text,
        n => format!("[{n} earlier lines dropped]\n{text}"),
    }
}

fn info(entry: &Entry, args: Option<&CreateArgs>) -> CreateInfo {
    let job = &entry.job;
    CreateInfo {
        job: job.id.clone(),
        id: entry.plane.clone(),
        args: args.cloned(),
        phase: match job.phase {
            Some(JobPhase::Fetching) => Some(Phase::Fetching),
            Some(JobPhase::AddingWorktrees) => Some(Phase::AddingWorktrees),
            Some(JobPhase::RunningScripts) => Some(Phase::RunningScripts),
            Some(JobPhase::Unknown) | None => None,
        },
        status: status(job),
        log: log_text(&entry.lines, entry.dropped),
        started_by: format!("{} {}", job.started_by.name, job.started_by.version),
        started_at: job.started_at,
    }
}

fn detail(job: &Job) -> PlaneCreateDetail {
    serde_json::from_value(job.detail.clone()).unwrap_or_default()
}

/// Mirror one job; a job whose plane isn't known yet isn't shown.
fn upsert(state: &mut State, job: Job) -> Option<CreateEvent> {
    let plane = detail(&job).plane?;
    let id = job.id.clone();
    let entry = match state.jobs.get_mut(&id) {
        Some(entry) if entry.job == job => return None,
        Some(entry) => {
            entry.job = job;
            entry.plane = plane;
            entry
        }
        None => state.jobs.entry(id.clone()).or_insert(Entry {
            job,
            plane,
            lines: Vec::new(),
            dropped: 0,
        }),
    };
    let entry: &Entry = entry;
    Some(CreateEvent {
        job: id.clone(),
        id: entry.plane.clone(),
        create: Some(info(entry, state.args.get(&id))),
    })
}

fn remove(state: &mut State, job: &str) -> Option<CreateEvent> {
    state.backlog_due.remove(job);
    let entry = state.jobs.remove(job)?;
    Some(CreateEvent {
        job: job.into(),
        id: entry.plane,
        create: None,
    })
}

fn params<T: DeserializeOwned>(params: Option<Value>) -> Option<T> {
    serde_json::from_value(params?).ok()
}

/// What `Jobs` subscribes with; weak, so the link doesn't keep it alive.
struct Subscription(Weak<Jobs>);

impl Subscriber for Subscription {
    fn connected(&self, handle: Handle) {
        if let Some(jobs) = self.0.upgrade() {
            jobs.connected(handle);
        }
    }

    fn event(&self, event: Notification) {
        if let Some(jobs) = self.0.upgrade() {
            jobs.event(event);
        }
    }

    fn frame(&self, _frame: Frame) {}
}

#[cfg(test)]
mod tests {
    use bitplane_protocol::job::JobClient;

    use super::*;

    fn args() -> CreateArgs {
        CreateArgs {
            id: "feat-x".into(),
            branch: "feat/x".into(),
            members: vec![
                Member {
                    spec: "@sos/api".into(),
                    branch: None,
                },
                Member {
                    spec: "/repos/web".into(),
                    branch: Some("main".into()),
                },
            ],
            intent: BranchIntent::Resolve,
            no_fetch: false,
            no_scripts: false,
        }
    }

    fn job(status: JobStatus, detail: Value) -> Job {
        Job {
            id: "j1".into(),
            kind: JobKind::PlaneCreate,
            status,
            phase: None,
            started_by: JobClient {
                name: "bitplane-cc".into(),
                version: "0.1.0".into(),
            },
            started_at: 1,
            finished_at: None,
            detail,
        }
    }

    fn envelope(error: &str) -> Value {
        json!({"error": error, "code": 1, "message": "m", "problems": [], "remedy": null})
    }

    #[test]
    fn the_request_is_bps_plane_create_request() {
        assert_eq!(
            request(&args()),
            json!({
                "members": ["@sos/api", "/repos/web:main"],
                "branch": "feat/x",
                "id": "feat-x",
                "here": null,
                "intent": "resolve",
                "fetch": true,
                "run_scripts": true,
            })
        );
        let mut all = args();
        all.intent = BranchIntent::RequireNew;
        all.no_fetch = true;
        all.no_scripts = true;
        let all = request(&all);
        assert_eq!(
            (&all["intent"], &all["fetch"], &all["run_scripts"]),
            (&json!("require_new"), &json!(false), &json!(false))
        );
    }

    #[test]
    fn no_fetch_is_dropped_with_resolve() {
        let mut auto = args();
        auto.no_fetch = true;
        assert_eq!(request(&auto)["fetch"], true);
    }

    #[test]
    fn the_status_is_the_jobs_with_bps_envelope() {
        let refused = json!({"plane": "x", "envelope": envelope("lock_timeout")});
        let CreateStatus::NothingCreated {
            envelope: Some(found),
            retryable: true,
        } = status(&job(JobStatus::NothingCreated, refused))
        else {
            panic!("a lock timeout is a retryable nothing-created");
        };
        assert_eq!(found.error, "lock_timeout");
        let aborted = json!({"plane": "x", "envelope": envelope("create_aborted")});
        assert!(matches!(
            status(&job(JobStatus::Partial, aborted)),
            CreateStatus::Partial { envelope: Some(_) }
        ));
        assert_eq!(
            status(&job(
                JobStatus::Failed {
                    reason: "daemon restarted".into()
                },
                json!({"plane": "x"})
            )),
            CreateStatus::Failed {
                reason: "daemon restarted".into()
            }
        );
        assert!(matches!(
            status(&job(JobStatus::Unknown, json!({}))),
            CreateStatus::Failed { .. }
        ));
    }

    #[test]
    fn a_job_without_a_plane_is_not_shown() {
        let mut state = State::default();
        assert_eq!(upsert(&mut state, job(JobStatus::Running, json!({}))), None);
        let event = upsert(&mut state, job(JobStatus::Running, json!({"plane": "x"}))).unwrap();
        assert_eq!(event.id, "x");
        let create = event.create.unwrap();
        assert_eq!(create.started_by, "bitplane-cc 0.1.0");
        assert_eq!(create.phase, None);
        assert_eq!(
            upsert(&mut state, job(JobStatus::Running, json!({"plane": "x"}))),
            None,
            "an unchanged job emits nothing"
        );
    }

    #[test]
    fn the_log_says_what_the_cap_dropped() {
        let lines = ["a".to_string(), "b".into()];
        assert_eq!(log_text(&lines, 0), "a\nb");
        assert_eq!(log_text(&lines, 3), "[3 earlier lines dropped]\na\nb");
    }
}
