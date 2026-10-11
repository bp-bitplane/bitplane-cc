//! bp calls and create jobs against a fake `bp` script and a temporary
//! planes directory.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bitplane_cc_lib::bp::{Bp, BpError, VersionError};
use bitplane_cc_lib::create::{
    BranchIntent, CreateArgs, CreateEvent, CreateStatus, Creates, Member, Phase,
};
use bitplane_cc_lib::env::EnvMap;

#[tokio::test]
async fn list_reads_findings_on_exit_3() {
    let fixture = Fixture::new(&[
        ("FAKE_BP_LIST_EXIT", "3"),
        (
            "FAKE_BP_LIST",
            r#"{"action":"plane_list","planes":[{"id":"a","directory":"/p/a","created_at":null,"members":[],"health":{"checked":"cheap","findings":[{"finding":"member_worktree_missing","member":"@x","path":"x"}]}}]}"#,
        ),
    ]);
    let planes = fixture.bp.list().await.unwrap();
    assert_eq!(planes.len(), 1);
    assert_eq!(planes[0].id, "a");
    assert_eq!(planes[0].directory, PathBuf::from("/p/a"));
    let findings = &planes[0].health.as_ref().unwrap().findings;
    assert_eq!(findings[0].finding, "member_worktree_missing");
}

#[tokio::test]
async fn list_reads_member_specs() {
    let fixture = Fixture::new(&[(
        "FAKE_BP_LIST",
        r#"{"action":"plane_list","planes":[{"id":"a","directory":"/p/a","created_at":null,"members":[{"member":"@sos/api","path":"sos/api","head":{"head":"branch","branch":"feat/x"}},{"member":"/repos/web","path":"projects/web","head":{"head":"detached"}}],"health":{"checked":"cheap","findings":[]}}]}"#,
    )]);
    let planes = fixture.bp.list().await.unwrap();
    let members: Vec<_> = planes[0]
        .members
        .iter()
        .map(|m| m.member.as_str())
        .collect();
    assert_eq!(members, ["@sos/api", "/repos/web"]);
}

#[tokio::test]
async fn every_call_passes_planes_dir_and_json() {
    let fixture = Fixture::new(&[]);
    fixture.bp.list().await.unwrap();
    let log = std::fs::read_to_string(&fixture.log).unwrap();
    let expected = format!("list --planes-dir {} --json", fixture.planes_dir.display());
    assert_eq!(log.trim(), expected);
}

#[tokio::test]
async fn git_exit_is_reported() {
    let fixture = Fixture::new(&[("FAKE_BP_LIST_EXIT", "4")]);
    assert_eq!(
        fixture.bp.list().await.unwrap_err(),
        BpError::Git("git is too old".into())
    );
}

#[tokio::test]
async fn unexpected_output_is_incompatible() {
    let fixture = Fixture::new(&[("FAKE_BP_LIST", r#"{"action":"plane_list"}"#)]);
    assert!(matches!(
        fixture.bp.list().await,
        Err(BpError::Incompatible(_))
    ));
}

#[tokio::test]
async fn version_check() {
    let ok = Fixture::new(&[]);
    assert_eq!(ok.bp.version().await, Ok("0.1.0".into()));
    let old = Fixture::new(&[("FAKE_BP_VERSION", "bp 0.0.9")]);
    assert_eq!(
        old.bp.version().await,
        Err(VersionError::TooOld("0.0.9".into()))
    );
    let odd = Fixture::new(&[("FAKE_BP_VERSION", "bitplane")]);
    assert!(matches!(
        odd.bp.version().await,
        Err(VersionError::Unparsable(_))
    ));
}

#[tokio::test]
async fn projects_report_missing_source_repos() {
    let fixture = Fixture::new(&[(
        "FAKE_BP_PROJECTS",
        r#"{"action":"project_list","projects":[{"project":"sos/api","outcome":"ok","detail":{"source":{"type":"owned","url":"u"},"source_repo_present":true}},{"project":"gone","outcome":"ok","detail":{"source":{"type":"adopted","path":"/x"},"source_repo_present":false}}]}"#,
    )]);
    let projects = fixture.bp.projects().await.unwrap();
    let summary: Vec<_> = projects
        .iter()
        .map(|p| (p.name.as_str(), p.source_repo_present))
        .collect();
    assert_eq!(summary, [("sos/api", true), ("gone", false)]);
}

#[tokio::test]
async fn create_succeeds() {
    let fixture = Fixture::new(&[("FAKE_BP_CREATE", "success")]);
    let (creates, events) = registry();
    creates.start(fixture.bp.clone(), args("feat-x")).unwrap();
    let status = wait_for_status(&events, "feat-x").await;
    assert_eq!(status, CreateStatus::Succeeded);
    let log = std::fs::read_to_string(&fixture.log).unwrap();
    assert!(log.contains("create @sos/api -b feat/x --id feat-x --planes-dir"));
    assert!(
        creates.sources().is_empty(),
        "a finished create leaves no placeholder"
    );
}

#[tokio::test]
async fn script_failure_is_a_success_with_a_log() {
    let fixture = Fixture::new(&[("FAKE_BP_CREATE", "script_failed")]);
    let (creates, events) = registry();
    creates.start(fixture.bp.clone(), args("feat-x")).unwrap();
    assert_eq!(
        wait_for_status(&events, "feat-x").await,
        CreateStatus::ScriptFailed
    );
    let info = creates.list().pop().unwrap();
    assert_eq!(info.log, "installing deps\nno newline at the end");
}

#[tokio::test]
async fn id_in_use_creates_nothing() {
    let fixture = Fixture::new(&[("FAKE_BP_CREATE", "in_use")]);
    std::fs::create_dir_all(fixture.planes_dir.join("feat-x")).unwrap();
    let (creates, events) = registry();
    creates.start(fixture.bp.clone(), args("feat-x")).unwrap();
    match wait_for_status(&events, "feat-x").await {
        CreateStatus::NothingCreated {
            envelope: Some(envelope),
            retryable: false,
        } => {
            assert_eq!(envelope.error, "plane_id_in_use");
            assert_eq!(envelope.remedy.as_deref(), Some("try again"));
        }
        other => panic!("unexpected {other:?}"),
    }
    creates.dismiss("feat-x");
    assert!(creates.list().is_empty());
    assert!(events.lock().unwrap().last().unwrap().create.is_none());
}

#[tokio::test]
async fn cancel_leaves_a_partial_plane() {
    let fixture = Fixture::new(&[("FAKE_BP_CREATE", "hang")]);
    let (creates, events) = registry();
    creates.start(fixture.bp.clone(), args("feat-x")).unwrap();

    let incomplete = fixture.planes_dir.join("feat-x/.bitplane/incomplete");
    wait_until(|| incomplete.exists()).await;
    creates.update_phases();
    assert_eq!(creates.list()[0].phase, Phase::AddingWorktrees);
    assert_eq!(creates.running_ids(), ["feat-x"]);
    assert_eq!(creates.sources().len(), 1);

    creates.cancel("feat-x");
    assert!(matches!(
        wait_for_status(&events, "feat-x").await,
        CreateStatus::Partial { .. }
    ));
    assert_eq!(creates.sources().len(), 1, "a partial plane keeps its row");
    assert!(creates.list()[0].log.contains("adding worktrees"));
}

#[tokio::test]
async fn a_second_create_with_the_same_id_is_refused_while_running() {
    let fixture = Fixture::new(&[("FAKE_BP_CREATE", "hang")]);
    let (creates, events) = registry();
    creates.start(fixture.bp.clone(), args("feat-x")).unwrap();
    assert!(creates.start(fixture.bp.clone(), args("feat-x")).is_err());
    creates.interrupt_all();
    wait_for_status(&events, "feat-x").await;
}

struct Fixture {
    bp: Bp,
    planes_dir: PathBuf,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new(vars: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let planes_dir = dir.path().join("planes");
        std::fs::create_dir_all(&planes_dir).unwrap();
        let log = dir.path().join("calls.log");
        let mut env: EnvMap = [
            ("PATH", "/usr/bin:/bin"),
            ("FAKE_BP_LOG", log.to_str().unwrap()),
        ]
        .into_iter()
        .chain(vars.iter().copied())
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        env.insert("HOME".into(), dir.path().to_string_lossy().into_owned());
        let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-bp");
        Fixture {
            bp: Bp {
                program,
                env,
                planes_dir: planes_dir.clone(),
            },
            planes_dir,
            log,
            _dir: dir,
        }
    }
}

type Events = Arc<Mutex<Vec<CreateEvent>>>;

fn registry() -> (Arc<Creates>, Events) {
    let events: Events = Arc::default();
    let sink = Arc::clone(&events);
    let creates = Creates::new(Arc::new(move |event| sink.lock().unwrap().push(event)));
    (creates, events)
}

fn args(id: &str) -> CreateArgs {
    CreateArgs {
        id: id.into(),
        branch: "feat/x".into(),
        members: vec![Member {
            spec: "@sos/api".into(),
            branch: None,
        }],
        intent: BranchIntent::Auto,
        no_fetch: false,
        no_scripts: false,
    }
}

async fn wait_for_status(events: &Events, id: &str) -> CreateStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let finished = events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|e| e.id == id)
            .find_map(|e| e.create.as_ref().map(|c| c.status.clone()))
            .filter(|s| *s != CreateStatus::Running);
        if let Some(status) = finished {
            return status;
        }
        assert!(Instant::now() < deadline, "create {id} never finished");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "condition never held");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
