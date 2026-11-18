//! Files, Source control and File tabs against a real bitplane-sessiond with
//! its fs and git modules, served in-process on a temp host.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use bitplane_cc_lib::daemon::{DaemonLink, DaemonStatus};
use bitplane_cc_lib::diff::{DiffMode, FileDiff, FileText};
use bitplane_cc_lib::files::{FileEntry, FileKind};
use bitplane_cc_lib::git::{ChangeKind, RepoState};
use bitplane_cc_lib::workspace::{
    FileTabsScope, FilesEvent, FilesScope, SourceControlEvent, SourceControlScope, WindowScope,
    Workspace, WorkspaceEmitters, path_string,
};
use bitplane_sessiond::env::EnvMap;
use bitplane_sessiond::paths::Dirs;
use bitplane_sessiond::server::{Config, serve};
use tokio::sync::{mpsc, oneshot};

const WAIT: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(1500);

#[tokio::test(flavor = "multi_thread")]
async fn lists_folders_with_their_ignore_flags() {
    let host = Host::new().await;
    host.write("web/.gitignore", "target/\n");
    host.write("web/target/app", "");
    host.write("web/src/main.rs", "");
    let cc = Cc::connect(&host).await;

    let listed = cc.workspace.list_dir(&host.plane, "web").await.unwrap();

    let entry = |name: &str, kind, ignored| FileEntry {
        name: name.into(),
        kind,
        ignored,
    };
    assert_eq!(
        listed,
        [
            entry(".git", FileKind::Directory, true),
            entry(".gitignore", FileKind::File, false),
            entry("a.txt", FileKind::File, false),
            entry("src", FileKind::Directory, false),
            entry("target", FileKind::Directory, true),
        ]
    );
    let missing = cc
        .workspace
        .list_dir(&host.plane, "nope")
        .await
        .unwrap_err();
    assert_eq!(missing, "nope no longer exists");
}

#[tokio::test(flavor = "multi_thread")]
async fn source_control_reads_every_repo_through_the_daemon() {
    let host = Host::new().await;
    host.write("web/a.txt", "one\nstaged\n");
    host.git(&["add", "a.txt"]);
    host.write("web/new.txt", "");
    let cc = Cc::connect(&host).await;

    let repos = cc
        .workspace
        .source_control(&host.plane, &["web".into(), "gone".into()])
        .await
        .unwrap();

    assert_eq!(repos[0].name, "web");
    let RepoState::Ready { status } = &repos[0].state else {
        panic!("{repos:?}");
    };
    assert_eq!(status.staged[0].kind, ChangeKind::Modified);
    assert_eq!(status.untracked, ["new.txt"]);
    assert_eq!(repos[1].state, RepoState::Missing);
}

#[tokio::test(flavor = "multi_thread")]
async fn file_tabs_diff_blobs_from_the_daemon() {
    let host = Host::new().await;
    host.write("web/a.txt", "one\nstaged\n");
    host.git(&["add", "a.txt"]);
    host.write("web/a.txt", "one\nstaged\nunstaged\n");
    host.write("web/new.txt", "fresh\n");
    host.write("notes.md", "# notes\n");
    let cc = Cc::connect(&host).await;
    let diff = |path: &'static str, mode| {
        let workspace = Arc::clone(&cc.workspace);
        let plane = host.plane.clone();
        async move {
            workspace
                .file_diff(&plane, path, mode, false)
                .await
                .unwrap()
        }
    };

    let index = patch(diff("web/a.txt", DiffMode::Index).await);
    assert!(index.contains("-two\n+staged\n"), "{index}");
    assert!(!index.contains("unstaged"), "{index}");
    let worktree = patch(diff("web/a.txt", DiffMode::Worktree).await);
    assert!(worktree.contains(" staged\n+unstaged\n"), "{worktree}");
    let file = patch(diff("web/a.txt", DiffMode::File).await);
    assert!(file.starts_with("diff --git a/a.txt b/a.txt\n"), "{file}");
    assert!(file.contains("-two\n+staged\n+unstaged\n"), "{file}");

    let added = patch(diff("web/new.txt", DiffMode::File).await);
    assert!(
        added.contains("+++ b/new.txt\n@@ -0,0 +1 @@\n+fresh\n"),
        "{added}"
    );
    assert_eq!(diff("web/new.txt", DiffMode::Index).await, FileDiff::None);
    assert_eq!(diff("notes.md", DiffMode::File).await, FileDiff::Plain);
    assert_eq!(
        cc.workspace
            .read_file(&host.plane, "notes.md")
            .await
            .unwrap(),
        FileText::Text {
            contents: "# notes\n".into()
        }
    );

    host.git(&["commit", "-qam", "both"]);
    assert_eq!(diff("web/a.txt", DiffMode::File).await, FileDiff::Plain);
    let contents = cc
        .workspace
        .file_contents(&host.plane, "web/a.txt", DiffMode::Index)
        .await
        .unwrap();
    assert_eq!(contents.old, contents.new);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_windows_get_only_their_own_events() {
    let host = Host::new().await;
    host.write("other/b.txt", "");
    let mut cc = Cc::connect(&host).await;
    let web = host.plane.join("web");
    let other = host.plane.join("other");
    cc.workspace.set_scope(
        "one",
        WindowScope {
            files: Some(FilesScope {
                plane_id: "demo".into(),
                dirs: vec![(path_string(&web), "web".into())],
            }),
            source_control: Some(SourceControlScope {
                plane_id: "demo".into(),
                root: host.plane.clone(),
                repos: vec![web.clone()],
            }),
            file_tabs: None,
        },
    );
    cc.workspace.set_scope(
        "two",
        WindowScope {
            file_tabs: Some(FileTabsScope {
                plane_id: "elsewhere".into(),
                folders: vec![path_string(&other)],
                repos: vec![],
            }),
            ..WindowScope::default()
        },
    );
    cc.settle().await;

    host.write("web/c.txt", "x");
    let (window, files) = cc.files().await;
    assert_eq!(window, "one");
    assert_eq!(files.dirs, ["web"]);
    let (window, source_control) = cc.source_control().await;
    assert_eq!(window, "one");
    assert_eq!(source_control.plane_id, "demo");
    assert!(cc.next_file_tab(QUIET).await.is_none());

    host.write("other/d.txt", "x");
    assert_eq!(
        cc.next_file_tab(WAIT).await,
        Some(("two".to_string(), "elsewhere".to_string()))
    );
    assert!(cc.next_files(QUIET).await.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vanished_folder_and_repo_stop_updating() {
    let host = Host::new().await;
    host.write("scratch/a.txt", "");
    let mut cc = Cc::connect(&host).await;
    let web = host.plane.join("web");
    let scratch = host.plane.join("scratch");
    cc.workspace.set_scope(
        "main",
        WindowScope {
            files: Some(FilesScope {
                plane_id: "demo".into(),
                dirs: vec![(path_string(&scratch), "scratch".into())],
            }),
            source_control: Some(SourceControlScope {
                plane_id: "demo".into(),
                root: host.plane.clone(),
                repos: vec![web.clone()],
            }),
            file_tabs: None,
        },
    );
    cc.settle().await;

    std::fs::remove_dir_all(&scratch).unwrap();
    std::fs::remove_dir_all(&web).unwrap();

    let (_, files) = cc.files().await;
    assert_eq!(files.dirs, ["scratch"]);
    loop {
        let (_, source_control) = cc.source_control().await;
        if source_control.repos[0].state == RepoState::Missing {
            break;
        }
    }
    cc.settle().await;
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::write(scratch.join("b.txt"), "").unwrap();
    assert!(cc.next_files(QUIET).await.is_none());
}

fn patch(diff: FileDiff) -> String {
    match diff {
        FileDiff::Diff { patch } => patch,
        other => panic!("expected a diff, got {other:?}"),
    }
}

/// A host with a plane holding one repo, `web`, with a committed `a.txt`.
struct Host {
    dirs: Dirs,
    plane: PathBuf,
    env: EnvMap,
    _root: tempfile::TempDir,
    _daemon: oneshot::Sender<()>,
}

impl Host {
    async fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cc")
            .tempdir_in("/tmp")
            .unwrap();
        let dirs = Dirs::for_version(&root.path().join("run"), &root.path().join("state"));
        let plane = root.path().join("demo");
        std::fs::create_dir_all(plane.join("web")).unwrap();
        let env = EnvMap::from([
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), root.path().display().to_string()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_AUTHOR_NAME".into(), "Test".into()),
            ("GIT_AUTHOR_EMAIL".into(), "test@example.com".into()),
            ("GIT_COMMITTER_NAME".into(), "Test".into()),
            ("GIT_COMMITTER_EMAIL".into(), "test@example.com".into()),
        ]);
        let (stop, stopped) = oneshot::channel::<()>();
        let config = Config {
            dirs: dirs.clone(),
            listener: None,
            // SAFETY: getuid has no preconditions.
            peer_uid: unsafe { libc::getuid() },
            modules: vec![
                bitplane_sessiond::fs::Fs::module(),
                bitplane_sessiond::git::Git::module(),
            ],
            env: Some(env.clone()),
        };
        tokio::spawn(serve(config, async move {
            let _ = stopped.await;
        }));
        while !dirs.socket().exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let host = Self {
            dirs,
            plane: plane.canonicalize().unwrap(),
            env,
            _root: root,
            _daemon: stop,
        };
        host.git(&["init", "-q", "-b", "main"]);
        host.write("web/a.txt", "one\ntwo\n");
        host.git(&["add", "."]);
        host.git(&["commit", "-q", "-m", "init"]);
        host
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.plane.join("web"))
            .env_clear()
            .envs(&self.env)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }

    fn write(&self, path: &str, contents: &str) {
        let path = self.plane.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}

type Routed<T> = mpsc::UnboundedReceiver<(String, T)>;

/// cc's backend workspace with every event it routes to a window.
struct Cc {
    _link: Arc<DaemonLink>,
    workspace: Arc<Workspace>,
    files: Routed<FilesEvent>,
    source_control: Routed<SourceControlEvent>,
    file_tabs: Routed<String>,
}

impl Cc {
    async fn connect(host: &Host) -> Self {
        let (connected_tx, mut connected) = mpsc::unbounded_channel();
        let link = DaemonLink::new(
            Ok(host.dirs.clone()),
            PathBuf::from("/usr/bin/true"),
            Arc::new(move |status| {
                if let DaemonStatus::Connected { .. } = status {
                    let _ = connected_tx.send(());
                }
            }),
        );
        let (files_tx, files) = mpsc::unbounded_channel();
        let (source_control_tx, source_control) = mpsc::unbounded_channel();
        let (file_tabs_tx, file_tabs) = mpsc::unbounded_channel();
        let workspace = Workspace::new(
            &link,
            WorkspaceEmitters {
                files: Arc::new(move |window, event| {
                    let _ = files_tx.send((window.to_string(), event));
                }),
                source_control: Arc::new(move |window, event| {
                    let _ = source_control_tx.send((window.to_string(), event));
                }),
                file_tabs: Arc::new(move |window, event| {
                    let _ = file_tabs_tx.send((window.to_string(), event.plane_id));
                }),
            },
        );
        link.start();
        tokio::time::timeout(WAIT, connected.recv())
            .await
            .expect("cc connects to the daemon");
        Self {
            _link: link,
            workspace,
            files,
            source_control,
            file_tabs,
        }
    }

    /// Let the watches land and drop the initial events they bring.
    async fn settle(&mut self) {
        tokio::time::sleep(QUIET).await;
        while self.files.try_recv().is_ok() {}
        while self.source_control.try_recv().is_ok() {}
        while self.file_tabs.try_recv().is_ok() {}
    }

    async fn files(&mut self) -> (String, FilesEvent) {
        self.next_files(WAIT).await.expect("a files_changed")
    }

    async fn source_control(&mut self) -> (String, SourceControlEvent) {
        tokio::time::timeout(WAIT, self.source_control.recv())
            .await
            .ok()
            .flatten()
            .expect("a source_control_changed")
    }

    async fn next_files(&mut self, wait: Duration) -> Option<(String, FilesEvent)> {
        tokio::time::timeout(wait, self.files.recv())
            .await
            .ok()
            .flatten()
    }

    async fn next_file_tab(&mut self, wait: Duration) -> Option<(String, String)> {
        tokio::time::timeout(wait, self.file_tabs.recv())
            .await
            .ok()
            .flatten()
    }
}
