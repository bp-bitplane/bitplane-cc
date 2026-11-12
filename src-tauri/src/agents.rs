//! The coding agents in this host's Claude terminals (ADR 0028) and the
//! agent kinds the daemon can launch, mirrored from its `agent.list` and its
//! events. A tab finds its agent by the session id, its `key`.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, Weak};

use bitplane_protocol::agent::{
    self as wire, AgentKindStatus, AgentListResult, AgentRef, AgentResult, Tracking,
};
use bitplane_protocol::{Frame, Notification, method};
use bitplane_sessiond::client::Handle;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::daemon::{DaemonLink, Subscriber};

/// One agent, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// The agent kind, such as `claude`.
    pub kind: String,
    /// The agent's own session id; what a resume names.
    pub agent_session: String,
    /// The daemon session it runs in, a tab's `key`; `None` once that closed.
    pub session: Option<String>,
    /// Where the agent session started; a resume starts it there again.
    pub cwd: String,
    pub name: Option<String>,
    pub state: AgentState,
    /// Why the last turn failed; the daemon clears it on the next prompt.
    pub last_error: Option<AgentError>,
    /// The agent's hooks were blocked by its settings, so its state comes
    /// from a poll and lags.
    pub polled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentState {
    Idle,
    Busy,
    Waiting {
        reason: WaitingReason,
    },
    Resuming,
    Exited,
    /// A state this build doesn't know, from a newer daemon.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WaitingReason {
    Permission,
    Input,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentError {
    /// Claude Code's error type, such as `rate_limit`.
    pub error: String,
    pub message: Option<String>,
}

/// `agent_changed`: one agent, or `None` once the daemon dropped it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub agent_session: String,
    pub agent: Option<AgentInfo>,
}

/// Whether an agent terminal of `kind` can be launched on this host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentKindInfo {
    pub kind: String,
    pub available: bool,
    /// Why it can't be launched, such as `claude` missing from `PATH`.
    pub error: Option<String>,
}

pub type EmitAgent = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Every kind, whenever one changes; a relist may also drop one.
pub type EmitKinds = Arc<dyn Fn(Vec<AgentKindInfo>) + Send + Sync>;

pub struct Agents {
    me: Weak<Agents>,
    emit: EmitAgent,
    emit_kinds: EmitKinds,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    agents: BTreeMap<String, AgentInfo>,
    kinds: BTreeMap<String, AgentKindInfo>,
    /// Counts connections; a list from an older one is dropped.
    connection: u64,
    /// While an `agent.list` is in flight: the agents and kinds the
    /// events touched meanwhile, which are newer than the list.
    listing: Option<Touched>,
}

#[derive(Default)]
struct Touched {
    agents: HashSet<String>,
    kinds: HashSet<String>,
}

impl Agents {
    /// Follows `link` from its next connection on.
    pub fn new(link: &DaemonLink, emit: EmitAgent, emit_kinds: EmitKinds) -> Arc<Self> {
        let agents = Arc::new_cyclic(|me| Agents {
            me: me.clone(),
            emit,
            emit_kinds,
            state: Mutex::default(),
        });
        link.subscribe(Arc::new(Subscription(Arc::downgrade(&agents))));
        agents
    }

    pub fn list(&self) -> Vec<AgentInfo> {
        self.state
            .lock()
            .unwrap()
            .agents
            .values()
            .cloned()
            .collect()
    }

    pub fn kinds(&self) -> Vec<AgentKindInfo> {
        self.state.lock().unwrap().kinds.values().cloned().collect()
    }

    fn connected(&self, handle: Handle) {
        let connection = {
            let mut state = self.state.lock().unwrap();
            state.connection += 1;
            state.listing = Some(Touched::default());
            state.connection
        };
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let listed = handle.call(method::AGENT_LIST, None).await;
            // A daemon without the agent module lists nothing.
            let listed = listed
                .ok()
                .and_then(|value| serde_json::from_value::<AgentListResult>(value).ok())
                .unwrap_or(AgentListResult {
                    kinds: Vec::new(),
                    agents: Vec::new(),
                });
            if let Some(agents) = me.upgrade() {
                agents.resync(connection, listed);
            }
        });
    }

    fn resync(&self, connection: u64, listed: AgentListResult) {
        let mut events = Vec::new();
        let kinds;
        {
            let mut state = self.state.lock().unwrap();
            if state.connection != connection {
                return;
            }
            let Touched {
                agents: touched,
                kinds: touched_kinds,
            } = state.listing.take().unwrap_or_default();
            let mut listed_kinds: BTreeMap<String, AgentKindInfo> = listed
                .kinds
                .into_iter()
                .map(kind_info)
                .map(|kind| (kind.kind.clone(), kind))
                .collect();
            for kind in &touched_kinds {
                match state.kinds.get(kind) {
                    Some(newer) => listed_kinds.insert(kind.clone(), newer.clone()),
                    None => listed_kinds.remove(kind),
                };
            }
            kinds = (state.kinds != listed_kinds).then(|| {
                state.kinds = listed_kinds;
                state.kinds.values().cloned().collect()
            });
            let listed: BTreeMap<String, AgentInfo> = listed
                .agents
                .into_iter()
                .map(info)
                .map(|agent| (agent.agent_session.clone(), agent))
                .collect();
            let gone: Vec<String> = state
                .agents
                .keys()
                .filter(|id| !listed.contains_key(*id) && !touched.contains(*id))
                .cloned()
                .collect();
            for id in gone {
                events.extend(remove(&mut state, &id));
            }
            for (id, agent) in listed {
                if !touched.contains(&id) {
                    events.extend(upsert(&mut state, agent));
                }
            }
        }
        self.emit_all(events);
        if let Some(kinds) = kinds {
            (self.emit_kinds)(kinds);
        }
    }

    fn event(&self, event: Notification) {
        let events = match event.method.as_str() {
            method::AGENT_CHANGED => {
                let Some(AgentResult { agent }) = params(event.params) else {
                    return;
                };
                let agent = info(agent);
                let mut state = self.state.lock().unwrap();
                if let Some(touched) = state.listing.as_mut() {
                    touched.agents.insert(agent.agent_session.clone());
                }
                upsert(&mut state, agent)
            }
            method::AGENT_REMOVED => {
                let Some(AgentRef { agent_session }) = params(event.params) else {
                    return;
                };
                let mut state = self.state.lock().unwrap();
                if let Some(touched) = state.listing.as_mut() {
                    touched.agents.insert(agent_session.clone());
                }
                remove(&mut state, &agent_session)
            }
            method::AGENT_KIND_CHANGED => {
                let Some(status) = params::<AgentKindStatus>(event.params) else {
                    return;
                };
                let kind = kind_info(status);
                let kinds = {
                    let mut state = self.state.lock().unwrap();
                    if let Some(touched) = state.listing.as_mut() {
                        touched.kinds.insert(kind.kind.clone());
                    }
                    if state.kinds.get(&kind.kind) == Some(&kind) {
                        return;
                    }
                    state.kinds.insert(kind.kind.clone(), kind);
                    state.kinds.values().cloned().collect()
                };
                (self.emit_kinds)(kinds);
                return;
            }
            _ => return,
        };
        self.emit_all(events);
    }

    fn emit_all(&self, events: impl IntoIterator<Item = AgentEvent>) {
        for event in events {
            (self.emit)(event);
        }
    }
}

/// What `Agents` subscribes with; weak, so the link doesn't keep it alive.
struct Subscription(Weak<Agents>);

impl Subscriber for Subscription {
    fn connected(&self, handle: Handle) {
        if let Some(agents) = self.0.upgrade() {
            agents.connected(handle);
        }
    }

    fn event(&self, event: Notification) {
        if let Some(agents) = self.0.upgrade() {
            agents.event(event);
        }
    }

    fn frame(&self, _frame: Frame) {}
}

fn upsert(state: &mut State, agent: AgentInfo) -> Option<AgentEvent> {
    if state.agents.get(&agent.agent_session) == Some(&agent) {
        return None;
    }
    state
        .agents
        .insert(agent.agent_session.clone(), agent.clone());
    Some(AgentEvent {
        agent_session: agent.agent_session.clone(),
        agent: Some(agent),
    })
}

fn remove(state: &mut State, agent_session: &str) -> Option<AgentEvent> {
    state.agents.remove(agent_session)?;
    Some(AgentEvent {
        agent_session: agent_session.into(),
        agent: None,
    })
}

fn kind_info(status: AgentKindStatus) -> AgentKindInfo {
    AgentKindInfo {
        kind: status.kind,
        available: status.available,
        error: status.error,
    }
}

fn info(agent: wire::Agent) -> AgentInfo {
    AgentInfo {
        kind: agent.kind,
        agent_session: agent.agent_session,
        session: agent.session,
        cwd: agent.cwd,
        name: agent.name,
        state: state(agent.state),
        last_error: agent.last_error.map(|e| AgentError {
            error: e.error,
            message: e.message,
        }),
        polled: agent.tracking == Tracking::Poll,
    }
}

fn state(state: wire::AgentState) -> AgentState {
    use wire::AgentState as Daemon;
    match state {
        Daemon::Idle => AgentState::Idle,
        Daemon::Busy => AgentState::Busy,
        Daemon::Waiting { reason } => AgentState::Waiting {
            reason: match reason {
                wire::WaitingReason::Permission => WaitingReason::Permission,
                wire::WaitingReason::Input => WaitingReason::Input,
                wire::WaitingReason::Other | wire::WaitingReason::Unknown => WaitingReason::Other,
            },
        },
        Daemon::Resuming => AgentState::Resuming,
        Daemon::Exited => AgentState::Exited,
        Daemon::Unknown => AgentState::Unknown,
    }
}

fn params<T: DeserializeOwned>(params: Option<Value>) -> Option<T> {
    serde_json::from_value(params?).ok()
}
