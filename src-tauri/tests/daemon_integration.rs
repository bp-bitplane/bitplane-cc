//! cc's connection to bitplane-sessiond, against the bundled daemon binary
//! and against a daemon served in-process with stub modules.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bitplane_cc_lib::daemon::{DaemonLink, DaemonStatus, ModuleStatus};
use bitplane_protocol::RpcError;
use bitplane_sessiond::env::EnvMap;
use bitplane_sessiond::module::{Availability, BoxFuture, Call, Module};
use bitplane_sessiond::paths::Dirs;
use bitplane_sessiond::pidfile::Pidfile;
use bitplane_sessiond::server::{Config, ServeError, serve};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const BUNDLED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/binaries/bitplane-sessiond-",
    env!("CC_TARGET_TRIPLE")
);

#[tokio::test]
async fn launching_starts_the_bundled_daemon() {
    let host = Host::new();
    let (link, mut statuses) = link(&host, Path::new(BUNDLED));

    link.start();

    let status = wait_for(&mut statuses, |s| {
        matches!(s, DaemonStatus::Connected { .. })
    })
    .await;
    let DaemonStatus::Connected { restarted, .. } = status else {
        unreachable!()
    };
    assert!(!restarted);
    assert!(host.dirs.pidfile().exists());
}

#[tokio::test]
async fn every_caller_shares_one_connection() {
    let host = Host::new();
    let _daemon = InProcess::serve(&host.dirs, vec![Arc::new(ConnectionId)]).await;
    let (link, mut statuses) = link(&host, no_daemon_exe());
    link.start();
    wait_for(&mut statuses, |s| {
        matches!(s, DaemonStatus::Connected { .. })
    })
    .await;

    let (window_a, window_b) = (link.handle().unwrap(), link.handle().unwrap());
    let (a, b) = tokio::join!(
        window_a.call("conn.id", None),
        window_b.call("conn.id", None)
    );

    assert_eq!(a.unwrap(), b.unwrap());
}

#[tokio::test]
async fn a_restarted_daemon_is_seen_as_a_new_instance() {
    let host = Host::new();
    let first = InProcess::serve(&host.dirs, vec![]).await;
    let (link, mut statuses) = link(&host, no_daemon_exe());
    link.start();
    let before = instance(wait_for(&mut statuses, is_connected).await);

    first.stop().await;
    wait_for(&mut statuses, |s| matches!(s, DaemonStatus::Connecting)).await;
    let _second = InProcess::serve(&host.dirs, vec![]).await;
    let status = wait_for(&mut statuses, is_connected).await;

    let DaemonStatus::Connected { restarted, .. } = &status else {
        unreachable!()
    };
    assert!(restarted);
    assert_ne!(instance(status), before);
}

#[tokio::test]
async fn an_unavailable_module_carries_its_reason() {
    let host = Host::new();
    let _daemon = InProcess::serve(&host.dirs, vec![Arc::new(Missing)]).await;
    let (link, mut statuses) = link(&host, no_daemon_exe());
    link.start();

    let status = wait_for(&mut statuses, is_connected).await;

    let DaemonStatus::Connected { modules, .. } = status else {
        unreachable!()
    };
    assert_eq!(
        modules,
        vec![ModuleStatus {
            module: "plane".into(),
            available: false,
            reason: Some("bp isn't on your PATH; install bitplane-cli".into()),
        }]
    );
}

#[tokio::test]
async fn a_daemon_that_never_answers_fails_with_its_log() {
    let host = Host::new();
    let (link, mut statuses) = link(&host, no_daemon_exe());
    link.start();

    let status = wait_for(&mut statuses, |s| matches!(s, DaemonStatus::Failed { .. })).await;

    let DaemonStatus::Failed { log, .. } = status else {
        unreachable!()
    };
    assert_eq!(log, Some(host.dirs.log().display().to_string()));
}

fn link(host: &Host, exe: &Path) -> (Arc<DaemonLink>, mpsc::UnboundedReceiver<DaemonStatus>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let link = DaemonLink::new(
        Ok(host.dirs.clone()),
        exe.to_path_buf(),
        Arc::new(move |status| {
            let _ = tx.send(status);
        }),
    );
    (link, rx)
}

/// Starting it does nothing, so only an in-process daemon ever answers.
fn no_daemon_exe() -> &'static Path {
    Path::new("/usr/bin/true")
}

async fn wait_for(
    statuses: &mut mpsc::UnboundedReceiver<DaemonStatus>,
    matches: impl Fn(&DaemonStatus) -> bool,
) -> DaemonStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let status = tokio::time::timeout(left, statuses.recv())
            .await
            .expect("timed out waiting for a daemon status")
            .expect("the link stopped");
        if matches(&status) {
            return status;
        }
    }
}

fn is_connected(status: &DaemonStatus) -> bool {
    matches!(status, DaemonStatus::Connected { .. })
}

fn instance(status: DaemonStatus) -> String {
    match status {
        DaemonStatus::Connected { instance, .. } => instance,
        other => panic!("not connected: {other:?}"),
    }
}

/// A host's directories in a short temp dir (macOS caps socket paths at 104
/// bytes). A daemon the bundled binary leaves behind is sent SIGTERM through
/// its pidfile, only while it still holds the lock.
struct Host {
    dirs: Dirs,
    _root: tempfile::TempDir,
}

impl Host {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cc")
            .tempdir_in("/tmp")
            .unwrap();
        let dirs = Dirs::for_version(&root.path().join("run"), &root.path().join("state"));
        Self { dirs, _root: root }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if Pidfile::acquire(&self.dirs.pidfile()).is_ok() {
            return;
        }
        let Some(pid) = std::fs::read_to_string(self.dirs.pidfile())
            .ok()
            .and_then(|pid| pid.trim().parse::<i32>().ok())
            .filter(|pid| *pid > 0 && *pid != std::process::id() as i32)
        else {
            return;
        };
        // SAFETY: kill has no memory-safety preconditions.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.dirs.socket().exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct InProcess {
    stop: oneshot::Sender<()>,
    task: JoinHandle<Result<(), ServeError>>,
}

impl InProcess {
    async fn serve(dirs: &Dirs, modules: Vec<Arc<dyn Module>>) -> Self {
        let (stop, stopped) = oneshot::channel::<()>();
        let config = Config {
            dirs: dirs.clone(),
            listener: None,
            // SAFETY: getuid has no preconditions.
            peer_uid: unsafe { libc::getuid() },
            modules,
            env: Some(EnvMap::new()),
        };
        let task = tokio::spawn(serve(config, async move {
            let _ = stopped.await;
        }));
        let socket: PathBuf = dirs.socket();
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Self { stop, task }
    }

    async fn stop(self) {
        let _ = self.stop.send(());
        self.task.await.unwrap().unwrap();
    }
}

/// `conn.id` answers with the id of the connection it was called on.
struct ConnectionId;

impl Module for ConnectionId {
    fn namespace(&self) -> &'static str {
        "conn"
    }

    fn probe(&self, _env: &EnvMap) -> Availability {
        Availability::Available { info: None }
    }

    fn handle(&self, call: Call) -> BoxFuture<'_, Result<Value, RpcError>> {
        Box::pin(async move { Ok(Value::from(call.connection.id)) })
    }
}

/// A module whose tool is missing, as `plane` is without `bp`.
struct Missing;

impl Module for Missing {
    fn namespace(&self) -> &'static str {
        "plane"
    }

    fn probe(&self, _env: &EnvMap) -> Availability {
        Availability::Unavailable {
            reason: "bp isn't on your PATH; install bitplane-cli".into(),
        }
    }

    fn handle(&self, call: Call) -> BoxFuture<'_, Result<Value, RpcError>> {
        Box::pin(async move { Err(RpcError::new(RpcError::METHOD_NOT_FOUND, call.method)) })
    }
}
