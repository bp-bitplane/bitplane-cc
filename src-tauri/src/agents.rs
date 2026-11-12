//! The coding agents in this host's Claude terminals (ADR 0028), mirrored
//! from the daemon's `agent.list` and its events. A tab finds its agent by
//! the session id, its `key`.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, Weak};

use bitplane_protocol::agent::{self as wire, AgentListResult, AgentRef, AgentResult, Tracking};
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

pub type EmitAgent = Arc<dyn Fn(AgentEvent) + Send + Sync>;

pub struct Agents {
    me: Weak<Agents>,
    emit: EmitAgent,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    agents: BTreeMap<String, AgentInfo>,
    /// Counts connections; a list from an older one is dropped.
    connection: u64,
    /// While an `agent.list` is in flight: the agents the events
    /// touched meanwhile, which are newer than the list.
    listing: Option<HashSet<String>>,
}

impl Agents {
    /// Follows `link` from its next connection on.
    pub fn new(link: &DaemonLink, emit: EmitAgent) -> Arc<Self> {
        let agents = Arc::new_cyclic(|me| Agents {
            me: me.clone(),
            emit,
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

    fn connected(&self, handle: Handle) {
        let connection = {
            let mut state = self.state.lock().unwrap();
            state.connection += 1;
            state.listing = Some(HashSet::new());
            state.connection
        };
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            let listed = handle.call(method::AGENT_LIST, None).await;
            // A daemon without the agent module lists nothing.
            let listed = listed
                .ok()
                .and_then(|value| serde_json::from_value::<AgentListResult>(value).ok())
                .map_or_else(Vec::new, |result| result.agents);
            if let Some(agents) = me.upgrade() {
                agents.resync(connection, listed);
            }
        });
    }

    fn resync(&self, connection: u64, listed: Vec<wire::Agent>) {
        let mut events = Vec::new();
        {
            let mut state = self.state.lock().unwrap();
            if state.connection != connection {
                return;
            }
            let touched = state.listing.take().unwrap_or_default();
            let listed: BTreeMap<String, AgentInfo> = listed
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
                    touched.insert(agent.agent_session.clone());
                }
                upsert(&mut state, agent)
            }
            method::AGENT_REMOVED => {
                let Some(AgentRef { agent_session }) = params(event.params) else {
                    return;
                };
                let mut state = self.state.lock().unwrap();
                if let Some(touched) = state.listing.as_mut() {
                    touched.insert(agent_session.clone());
                }
                remove(&mut state, &agent_session)
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
