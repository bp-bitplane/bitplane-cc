//! cc's one connection to this host's bitplane-sessiond, shared by every
//! window. It starts the bundled daemon when none answers and reconnects when
//! the connection drops.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bitplane_protocol::HelloResult;
use bitplane_sessiond::autostart::{StartError, connect_or_start};
use bitplane_sessiond::client::{Handle, Streams};
use bitplane_sessiond::paths::Dirs;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Notify;

/// The client name cc gives in `hello`.
pub const CLIENT_NAME: &str = "bitplane-cc";

const RETRY_MIN: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DaemonStatus {
    Connecting,
    Connected {
        version: String,
        instance: String,
        /// The daemon is a different instance from the one cc was last
        /// connected to; a reconnect to the same one is a blip.
        restarted: bool,
        modules: Vec<ModuleStatus>,
    },
    Failed {
        message: String,
        /// The daemon's log, when it was started and never answered.
        log: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModuleStatus {
    pub module: String,
    pub available: bool,
    /// Why it is unavailable, naming the fix.
    pub reason: Option<String>,
}

pub type OnChange = Arc<dyn Fn(DaemonStatus) + Send + Sync>;

pub struct DaemonLink {
    dirs: Result<Dirs, String>,
    exe: PathBuf,
    status: Mutex<DaemonStatus>,
    handle: Mutex<Option<Handle>>,
    retry: Notify,
    on_change: OnChange,
}

impl DaemonLink {
    /// `exe` is the daemon binary to start when none answers.
    pub fn new(dirs: Result<Dirs, String>, exe: PathBuf, on_change: OnChange) -> Arc<Self> {
        Arc::new(Self {
            dirs,
            exe,
            status: Mutex::new(DaemonStatus::Connecting),
            handle: Mutex::new(None),
            retry: Notify::new(),
            on_change,
        })
    }

    /// Runs the connection for the rest of the app's life. Needs a tokio
    /// runtime.
    pub fn start(self: &Arc<Self>) {
        tokio::spawn(Arc::clone(self).supervise());
    }

    pub fn status(&self) -> DaemonStatus {
        self.status.lock().unwrap().clone()
    }

    /// The live connection, while there is one.
    pub fn handle(&self) -> Option<Handle> {
        self.handle.lock().unwrap().clone()
    }

    /// Tries again at once instead of waiting out the backoff.
    pub fn retry(&self) {
        self.retry.notify_one();
    }

    async fn supervise(self: Arc<Self>) {
        let mut previous: Option<String> = None;
        let mut delay = RETRY_MIN;
        loop {
            match self.connect().await {
                Ok((handle, mut streams, hello)) => {
                    let instance = hello.daemon.instance.clone();
                    let restarted = previous.as_ref().is_some_and(|last| *last != instance);
                    previous = Some(instance);
                    delay = RETRY_MIN;
                    *self.handle.lock().unwrap() = Some(handle);
                    self.set_status(connected(hello, restarted));
                    while streams.next_event().await.is_some() {}
                    self.handle.lock().unwrap().take();
                    self.set_status(DaemonStatus::Connecting);
                }
                // Stays failed while retrying, so the banner doesn't flicker.
                Err(failed) => {
                    self.set_status(failed);
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = self.retry.notified() => {}
                    }
                    delay = (delay * 2).min(RETRY_MAX);
                }
            }
        }
    }

    async fn connect(&self) -> Result<(Handle, Streams, HelloResult), DaemonStatus> {
        let dirs = self
            .dirs
            .as_ref()
            .map_err(|message| failed(message, None))?;
        let client = connect_or_start(dirs, &self.exe)
            .await
            .map_err(|error| match &error {
                StartError::Timeout { log } => {
                    failed(&error.to_string(), Some(log.display().to_string()))
                }
                StartError::Spawn(_) => failed(&error.to_string(), None),
            })?;
        let hello = client
            .hello(CLIENT_NAME)
            .await
            .map_err(|error| failed(&format!("the daemon refused cc: {error}"), None))?;
        let (handle, streams) = client.split();
        Ok((handle, streams, hello))
    }

    fn set_status(&self, status: DaemonStatus) {
        let mut current = self.status.lock().unwrap();
        if *current != status {
            *current = status.clone();
            drop(current);
            (self.on_change)(status);
        }
    }
}

fn connected(hello: HelloResult, restarted: bool) -> DaemonStatus {
    DaemonStatus::Connected {
        version: hello.daemon.version,
        instance: hello.daemon.instance,
        restarted,
        modules: hello
            .capabilities
            .into_iter()
            .map(|capability| ModuleStatus {
                module: capability.module,
                available: capability.available,
                reason: capability.reason,
            })
            .collect(),
    }
}

fn failed(message: &str, log: Option<String>) -> DaemonStatus {
    DaemonStatus::Failed {
        message: message.into(),
        log,
    }
}
