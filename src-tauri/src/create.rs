//! Create jobs: `bp create` with a live log, a phase read off the filesystem,
//! Cancel, and an outcome decided by what's left on disk (spec §5.3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::AsyncReadExt;

use crate::bp::{self, Bp, Envelope, PlaneCreated};
use crate::planes::{BITPLANE_DIR, PLANE_FILE, Source};

/// Bytes of log kept per create; older output is dropped from the front.
const LOG_LIMIT: usize = 512 * 1024;
/// How often a running create may emit its log.
const EMIT_INTERVAL: Duration = Duration::from_millis(150);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BranchIntent {
    Auto,
    New,
    Existing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Fetching,
    AddingWorktrees,
    RunningScripts,
}

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
}

/// The full state of one create, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateInfo {
    pub id: String,
    pub args: CreateArgs,
    pub directory: String,
    pub phase: Phase,
    pub status: CreateStatus,
    pub log: String,
}

/// `create_changed`: one create, or `None` once it's dismissed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CreateEvent {
    pub id: String,
    pub create: Option<CreateInfo>,
}

pub type EmitCreate = Arc<dyn Fn(CreateEvent) + Send + Sync>;

/// Every create the app has started and not yet dismissed.
pub struct Creates {
    jobs: Mutex<BTreeMap<String, Job>>,
    emit: EmitCreate,
}

struct Job {
    info: CreateInfo,
    pid: Option<u32>,
    last_emit: Instant,
}

impl Creates {
    pub fn new(emit: EmitCreate) -> Arc<Self> {
        Arc::new(Creates {
            jobs: Mutex::new(BTreeMap::new()),
            emit,
        })
    }

    /// Spawn `bp create` and track it until it's dismissed.
    pub fn start(self: &Arc<Self>, bp: Bp, args: CreateArgs) -> Result<(), String> {
        let directory = bp.planes_dir.join(&args.id);
        let mut child = bp
            .command(argv(&args))
            .spawn()
            .map_err(|e| format!("couldn't run bp: {e}"))?;
        let info = CreateInfo {
            id: args.id.clone(),
            args,
            directory: directory.to_string_lossy().into_owned(),
            phase: Phase::Fetching,
            status: CreateStatus::Running,
            log: String::new(),
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs
                .get(&info.id)
                .is_some_and(|job| job.info.status == CreateStatus::Running)
            {
                return Err(format!("{} is already being created", info.id));
            }
            jobs.insert(
                info.id.clone(),
                Job {
                    info: info.clone(),
                    pid: child.id(),
                    last_emit: Instant::now(),
                },
            );
        }
        self.emit_one(&info.id);

        let creates = Arc::clone(self);
        let id = info.id.clone();
        tokio::spawn(async move {
            let mut stdout = child.stdout.take().expect("stdout is piped");
            let mut stderr = child.stderr.take().expect("stderr is piped");
            let read_stdout = async {
                let mut bytes = Vec::new();
                let _ = stdout.read_to_end(&mut bytes).await;
                bytes
            };
            let read_stderr = async {
                let mut all = Vec::new();
                let mut buf = vec![0u8; 8192];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            all.extend_from_slice(&buf[..n]);
                            creates.append_log(&id, &String::from_utf8_lossy(&buf[..n]));
                        }
                    }
                }
                all
            };
            let (stdout_bytes, stderr_bytes) = tokio::join!(read_stdout, read_stderr);
            let code = child.wait().await.ok().and_then(|status| status.code());
            let output = bp::finish(code, &stdout_bytes, &String::from_utf8_lossy(&stderr_bytes));
            creates.finish(&id, output, &directory);
        });
        Ok(())
    }

    /// Send SIGINT; bp unwinds and reports `interrupted`.
    pub fn cancel(&self, id: &str) {
        let jobs = self.jobs.lock().unwrap();
        if let Some(Job {
            pid: Some(pid),
            info,
            ..
        }) = jobs.get(id)
            && info.status == CreateStatus::Running
        {
            interrupt(*pid);
        }
    }

    /// Forget a finished create.
    pub fn dismiss(&self, id: &str) {
        let removed = {
            let mut jobs = self.jobs.lock().unwrap();
            match jobs.get(id) {
                Some(job) if job.info.status != CreateStatus::Running => jobs.remove(id),
                _ => None,
            }
        };
        if removed.is_some() {
            (self.emit)(CreateEvent {
                id: id.into(),
                create: None,
            });
        }
    }

    /// Re-read each running create's phase from disk; phases only move forward.
    pub fn update_phases(&self) {
        let changed: Vec<String> = {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.values_mut()
                .filter(|job| job.info.status == CreateStatus::Running)
                .filter_map(|job| {
                    let phase = phase_on_disk(Path::new(&job.info.directory));
                    (phase > job.info.phase).then(|| {
                        job.info.phase = phase;
                        job.info.id.clone()
                    })
                })
                .collect()
        };
        for id in changed {
            self.emit_one(&id);
        }
    }

    pub fn list(&self) -> Vec<CreateInfo> {
        self.jobs
            .lock()
            .unwrap()
            .values()
            .map(|job| job.info.clone())
            .collect()
    }

    /// Creates that still occupy a sidebar row: running, or failed partway.
    pub fn sources(&self) -> Vec<Source> {
        self.jobs
            .lock()
            .unwrap()
            .values()
            .filter(|job| {
                matches!(
                    job.info.status,
                    CreateStatus::Running | CreateStatus::Partial { .. }
                )
            })
            .map(|job| Source::new(&job.info.id, &job.info.directory))
            .collect()
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.jobs
            .lock()
            .unwrap()
            .values()
            .filter(|job| job.info.status == CreateStatus::Running)
            .map(|job| job.info.id.clone())
            .collect()
    }

    /// SIGINT every running create (quitting).
    pub fn interrupt_all(&self) {
        for job in self.jobs.lock().unwrap().values() {
            if let (Some(pid), CreateStatus::Running) = (job.pid, &job.info.status) {
                interrupt(pid);
            }
        }
    }

    fn append_log(&self, id: &str, text: &str) {
        let due = {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(id) else { return };
            job.info.log.push_str(text);
            trim_front(&mut job.info.log, LOG_LIMIT);
            let due = job.last_emit.elapsed() >= EMIT_INTERVAL;
            if due {
                job.last_emit = Instant::now();
            }
            due
        };
        if due {
            self.emit_one(id);
        }
    }

    fn finish(&self, id: &str, output: bp::Output, directory: &Path) {
        {
            let mut jobs = self.jobs.lock().unwrap();
            let Some(job) = jobs.get_mut(id) else { return };
            let created: Option<PlaneCreated> = serde_json::from_str(output.stdout.trim()).ok();
            job.info.status = classify(
                output.code,
                created.as_ref(),
                output.envelope.as_ref(),
                directory.exists(),
            );
            job.info.log = output.log;
            trim_front(&mut job.info.log, LOG_LIMIT);
            job.pid = None;
        }
        self.emit_one(id);
    }

    fn emit_one(&self, id: &str) {
        let info = self
            .jobs
            .lock()
            .unwrap()
            .get(id)
            .map(|job| job.info.clone());
        if let Some(info) = info {
            (self.emit)(CreateEvent {
                id: id.into(),
                create: Some(info),
            });
        }
    }
}

/// The `bp create` arguments (the spawner adds `--planes-dir` and `--json`).
pub fn argv(args: &CreateArgs) -> Vec<String> {
    let mut argv = vec!["create".to_string()];
    for member in &args.members {
        argv.push(match member.branch.as_deref().filter(|b| !b.is_empty()) {
            Some(branch) => format!("{}:{branch}", member.spec),
            None => member.spec.clone(),
        });
    }
    argv.extend([
        "-b".into(),
        args.branch.clone(),
        "--id".into(),
        args.id.clone(),
    ]);
    match args.intent {
        BranchIntent::Auto => {}
        BranchIntent::New => argv.push("--new-branch".into()),
        BranchIntent::Existing => argv.push("--existing-branch".into()),
    }
    // bp refuses --no-fetch with its default intent, so the dialog only
    // offers it with New or Existing.
    if args.no_fetch && args.intent != BranchIntent::Auto {
        argv.push("--no-fetch".into());
    }
    if args.no_scripts {
        argv.push("--no-scripts".into());
    }
    argv
}

/// The phase bp has reached, judging by its files: the plane directory is
/// claimed after the fetch, and the `incomplete` latch is removed once every
/// worktree is added, before scripts run.
pub fn phase_on_disk(directory: &Path) -> Phase {
    let incomplete: PathBuf = directory.join(BITPLANE_DIR).join("incomplete");
    if !directory.exists() {
        Phase::Fetching
    } else if incomplete.exists() || !directory.join(PLANE_FILE).exists() {
        Phase::AddingWorktrees
    } else {
        Phase::RunningScripts
    }
}

/// Decide a finished create's outcome.
pub fn classify(
    code: Option<i32>,
    created: Option<&PlaneCreated>,
    envelope: Option<&Envelope>,
    directory_exists: bool,
) -> CreateStatus {
    if code == Some(bp::EXIT_OK) && created.is_some_and(|c| !c.interrupted && !c.remnant) {
        return CreateStatus::Succeeded;
    }
    let error = envelope.map(|e| e.error.as_str());
    if error == Some("script_failed") {
        return CreateStatus::ScriptFailed;
    }
    // Exit 2 can mean the id is taken, and that directory isn't ours.
    let before_claim = matches!(code, Some(bp::EXIT_USAGE | bp::EXIT_LOCK_TIMEOUT))
        || error == Some("fetch_failed");
    if directory_exists && !before_claim {
        return CreateStatus::Partial {
            envelope: envelope.cloned(),
        };
    }
    CreateStatus::NothingCreated {
        envelope: envelope.cloned(),
        retryable: code == Some(bp::EXIT_LOCK_TIMEOUT),
    }
}

fn interrupt(pid: u32) {
    // SAFETY: plain kill(2) on a child we spawned.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGINT);
    }
}

fn trim_front(log: &mut String, limit: usize) {
    if log.len() > limit {
        let mut cut = log.len() - limit;
        while !log.is_char_boundary(cut) {
            cut += 1;
        }
        log.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
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
            intent: BranchIntent::Auto,
            no_fetch: false,
            no_scripts: false,
        }
    }

    fn envelope(error: &str, code: i32) -> Envelope {
        Envelope {
            error: error.into(),
            code,
            message: "m".into(),
            problems: vec![],
            remedy: None,
        }
    }

    fn created(interrupted: bool, remnant: bool) -> PlaneCreated {
        PlaneCreated {
            id: "feat-x".into(),
            directory: "/p/feat-x".into(),
            interrupted,
            remnant,
        }
    }

    #[test]
    fn argv_covers_every_option() {
        assert_eq!(
            argv(&args()),
            [
                "create",
                "@sos/api",
                "/repos/web:main",
                "-b",
                "feat/x",
                "--id",
                "feat-x"
            ]
        );
        let mut all = args();
        all.intent = BranchIntent::New;
        all.no_fetch = true;
        all.no_scripts = true;
        assert_eq!(
            argv(&all)[7..],
            ["--new-branch", "--no-fetch", "--no-scripts"]
        );
        let mut existing = args();
        existing.intent = BranchIntent::Existing;
        assert_eq!(argv(&existing)[7..], ["--existing-branch"]);
    }

    #[test]
    fn no_fetch_is_dropped_with_auto_intent() {
        let mut auto = args();
        auto.no_fetch = true;
        assert!(!argv(&auto).contains(&"--no-fetch".to_string()));
    }

    #[test]
    fn phase_follows_the_files() {
        let dir = tempfile::tempdir().unwrap();
        let plane = dir.path().join("p");
        assert_eq!(phase_on_disk(&plane), Phase::Fetching);
        std::fs::create_dir_all(plane.join(".bitplane")).unwrap();
        std::fs::write(plane.join(".bitplane/incomplete"), "").unwrap();
        assert_eq!(phase_on_disk(&plane), Phase::AddingWorktrees);
        std::fs::write(plane.join(PLANE_FILE), "").unwrap();
        assert_eq!(phase_on_disk(&plane), Phase::AddingWorktrees);
        std::fs::remove_file(plane.join(".bitplane/incomplete")).unwrap();
        assert_eq!(phase_on_disk(&plane), Phase::RunningScripts);
    }

    #[test]
    fn classifies_outcomes() {
        assert_eq!(
            classify(Some(0), Some(&created(false, false)), None, true),
            CreateStatus::Succeeded
        );
        assert_eq!(
            classify(Some(1), None, Some(&envelope("script_failed", 1)), true),
            CreateStatus::ScriptFailed
        );
        let in_use = envelope("plane_id_in_use", 2);
        assert_eq!(
            classify(Some(2), None, Some(&in_use), true),
            CreateStatus::NothingCreated {
                envelope: Some(in_use),
                retryable: false
            }
        );
        assert!(matches!(
            classify(Some(1), None, Some(&envelope("fetch_failed", 1)), false),
            CreateStatus::NothingCreated {
                retryable: false,
                ..
            }
        ));
        assert!(matches!(
            classify(Some(5), None, Some(&envelope("lock_timeout", 5)), false),
            CreateStatus::NothingCreated {
                retryable: true,
                ..
            }
        ));
        assert!(matches!(
            classify(Some(1), None, Some(&envelope("create_aborted", 1)), false),
            CreateStatus::NothingCreated { .. }
        ));
        assert!(matches!(
            classify(Some(1), None, Some(&envelope("create_aborted", 1)), true),
            CreateStatus::Partial { .. }
        ));
        assert!(matches!(
            classify(Some(130), Some(&created(true, false)), None, true),
            CreateStatus::Partial { envelope: None }
        ));
        assert!(matches!(
            classify(Some(130), Some(&created(true, false)), None, false),
            CreateStatus::NothingCreated { .. }
        ));
    }

    #[test]
    fn log_is_trimmed_on_a_char_boundary() {
        let mut log = "ééééé".to_string();
        trim_front(&mut log, 5);
        assert!(log.len() <= 6);
        assert!(log.chars().all(|c| c == 'é'));
    }
}
