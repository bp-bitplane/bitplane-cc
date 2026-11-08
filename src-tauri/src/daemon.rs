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

/// Where the daemon runs, for the daemon info dialog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DaemonInfo {
    /// The last `hello` reply as pretty JSON; kept after a drop.
    pub hello: Option<String>,
    /// From the pidfile, which only the lock holder writes.
    pub pid: Option<u32>,
    /// The running daemon's executable, which may not be cc's bundled one.
    pub executable: Option<String>,
    pub bundled: String,
    pub socket: Option<String>,
    pub log: Option<String>,
}

pub type OnChange = Arc<dyn Fn(DaemonStatus) + Send + Sync>;

pub struct DaemonLink {
    dirs: Result<Dirs, String>,
    exe: PathBuf,
    status: Mutex<DaemonStatus>,
    handle: Mutex<Option<Handle>>,
    hello: Mutex<Option<HelloResult>>,
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
            hello: Mutex::new(None),
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

    /// The last `hello` reply, as the daemon sent it; kept after a drop.
    pub fn hello(&self) -> Option<HelloResult> {
        self.hello.lock().unwrap().clone()
    }

    pub fn info(&self) -> DaemonInfo {
        let dirs = self.dirs.as_ref().ok();
        let pid = dirs
            .and_then(|dirs| std::fs::read_to_string(dirs.pidfile()).ok())
            .and_then(|pid| pid.trim().parse().ok());
        DaemonInfo {
            hello: self
                .hello()
                .and_then(|hello| serde_json::to_string_pretty(&hello).ok()),
            pid,
            executable: pid.and_then(executable_of),
            bundled: self.exe.display().to_string(),
            socket: dirs.map(|dirs| dirs.socket().display().to_string()),
            log: dirs.map(|dirs| dirs.log().display().to_string()),
        }
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
                    *self.hello.lock().unwrap() = Some(hello.clone());
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

#[cfg(target_os = "macos")]
fn executable_of(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: proc_pidpath writes at most `buf.len()` bytes into `buf`.
    let len = unsafe { libc::proc_pidpath(pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn executable_of(pid: u32) -> Option<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    Some(path.display().to_string())
}
