import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { X } from "lucide-react";
import { type FormEvent, useEffect, useId, useMemo, useState } from "react";
import { DialogActions, Modal } from "../dialogs/Modal";
import { type BranchIntent, type CreateArgs, commands, type Member, type Project } from "../ipc";
import {
  type CreateFailure,
  closeDialog,
  createDraft,
  createPlane,
  useStore,
} from "../store/store";
import styles from "./CreatePlaneDialog.module.css";
import { EnvelopeDetails, Log } from "./CreateProgress";
import { idFromBranch, planeIdProblem } from "./planeId";

type Tab = "empty" | "projects";

/** Create plane (spec §5.3): an empty draft plane, or `bp create` from projects. */
export function CreatePlaneDialog({
  initial,
  failure,
}: {
  initial: CreateArgs | null;
  failure: CreateFailure | null;
}) {
  const [tab, setTab] = useState<Tab>(initial ? "projects" : "empty");
  return (
    <Modal label="Create plane" onClose={closeDialog} wide>
      <div className={styles.dialog}>
        <h3>Create plane</h3>
        <div className={styles.tabs} role="tablist">
          <TabButton tab="empty" current={tab} onSelect={setTab}>
            Empty
          </TabButton>
          <TabButton tab="projects" current={tab} onSelect={setTab}>
            From projects
          </TabButton>
        </div>
        {tab === "empty" ? <EmptyForm /> : <ProjectsForm initial={initial} failure={failure} />}
      </div>
    </Modal>
  );
}

function EmptyForm() {
  const existing = useExistingIds();
  const [id, setId] = useState("");
  const [touched, setTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const problem = planeIdProblem(id, existing);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (problem) return;
    setBusy(true);
    try {
      await createDraft(id);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className={styles.form}>
      <p className="muted">
        Makes an empty folder in the planes directory and opens a session in it. Turn it into a
        plane later with <code>bp create --here</code>.
      </p>
      <Field label="Id" problem={touched || id ? problem : null}>
        {(inputId) => (
          <input
            id={inputId}
            // biome-ignore lint/a11y/noAutofocus: the first field of a dialog the user just opened
            autoFocus
            value={id}
            spellCheck={false}
            autoCapitalize="off"
            onChange={(e) => {
              setId(e.target.value);
              setError(null);
            }}
          />
        )}
      </Field>
      {error && <p className={styles.error}>{error}</p>}
      <DialogActions>
        <button type="button" onClick={closeDialog}>
          Cancel
        </button>
        <button type="submit" className="primary" disabled={busy || problem !== null}>
          Create
        </button>
      </DialogActions>
    </form>
  );
}

interface ChosenMember {
  /** `@project` or a repo path. */
  spec: string;
  branch: string;
}

function ProjectsForm({
  initial,
  failure,
}: {
  initial: CreateArgs | null;
  failure: CreateFailure | null;
}) {
  const existing = useExistingIds();
  const projects = useProjects();
  const [branch, setBranch] = useState(initial?.branch ?? "");
  const [members, setMembers] = useState<ChosenMember[]>(
    initial?.members.map((m) => ({ spec: m.spec, branch: m.branch ?? "" })) ?? [],
  );
  const [id, setId] = useState(initial?.id ?? "");
  const [idEdited, setIdEdited] = useState(initial !== null);
  const [intent, setIntent] = useState<BranchIntent>(initial?.intent ?? "auto");
  const [noFetch, setNoFetch] = useState(initial?.noFetch ?? false);
  const [noScripts, setNoScripts] = useState(initial?.noScripts ?? false);
  const [search, setSearch] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const effectiveId = idEdited ? id : idFromBranch(branch);
  const idProblem = planeIdProblem(effectiveId, existing);
  const branchProblem = branch.trim() === "" ? "Enter a branch" : null;
  const membersProblem = members.length === 0 ? "Pick at least one project or path" : null;
  const invalid = idProblem ?? branchProblem ?? membersProblem;

  const chosen = new Set(members.map((m) => m.spec));
  const toggleProject = (project: Project) => {
    const spec = `@${project.name}`;
    setMembers((current) =>
      chosen.has(spec)
        ? current.filter((m) => m.spec !== spec)
        : [...current, { spec, branch: "" }],
    );
  };
  const addPath = async () => {
    const picked = await openFolderPicker({ directory: true, multiple: false });
    if (typeof picked === "string" && !chosen.has(picked)) {
      setMembers((current) => [...current, { spec: picked, branch: "" }]);
    }
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (invalid) return;
    const args: CreateArgs = {
      id: effectiveId,
      branch: branch.trim(),
      members: members.map(
        (m): Member => ({ spec: m.spec, branch: m.branch.trim() === "" ? null : m.branch.trim() }),
      ),
      intent,
      noFetch: intent !== "auto" && noFetch,
      noScripts,
    };
    setBusy(true);
    try {
      await createPlane(args);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  };

  const visibleProjects =
    projects.kind === "loaded"
      ? projects.projects.filter((p) => p.name.toLowerCase().includes(search.trim().toLowerCase()))
      : [];

  return (
    <form onSubmit={submit} className={styles.form}>
      {failure && <FailureDetails failure={failure} />}
      <Field label="Branch" problem={submitted ? branchProblem : null}>
        {(inputId) => (
          <input
            id={inputId}
            // biome-ignore lint/a11y/noAutofocus: the first field of a dialog the user just opened
            autoFocus={!initial}
            value={branch}
            placeholder="feat/login"
            spellCheck={false}
            autoCapitalize="off"
            onChange={(e) => setBranch(e.target.value)}
          />
        )}
      </Field>

      <fieldset className={styles.fieldset}>
        <legend>Projects</legend>
        {members.length > 0 && (
          <ul className={styles.chips} aria-label="Members">
            {members.map((member) => (
              <li key={member.spec} className={styles.chip}>
                <span className={styles.chipName} title={member.spec}>
                  {member.spec}
                </span>
                <input
                  className={styles.chipBranch}
                  placeholder={branch || "branch"}
                  aria-label={`Branch for ${member.spec}`}
                  value={member.branch}
                  spellCheck={false}
                  onChange={(e) =>
                    setMembers((current) =>
                      current.map((m) =>
                        m.spec === member.spec ? { ...m, branch: e.target.value } : m,
                      ),
                    )
                  }
                />
                <button
                  type="button"
                  aria-label={`Remove ${member.spec}`}
                  onClick={() =>
                    setMembers((current) => current.filter((m) => m.spec !== member.spec))
                  }
                >
                  <X size={12} />
                </button>
              </li>
            ))}
          </ul>
        )}
        <input
          placeholder="Search projects…"
          aria-label="Search projects"
          value={search}
          spellCheck={false}
          onChange={(e) => setSearch(e.target.value)}
        />
        <div className={styles.projects}>
          {projects.kind === "loading" && <p className="muted">Loading projects…</p>}
          {projects.kind === "failed" && <p className={styles.error}>{projects.error}</p>}
          {projects.kind === "loaded" && visibleProjects.length === 0 && (
            <p className="muted">No projects match</p>
          )}
          {visibleProjects.map((project) => (
            <label
              key={project.name}
              className={styles.project}
              title={project.sourceRepoPresent ? undefined : "Its source repo is missing"}
            >
              <input
                type="checkbox"
                disabled={!project.sourceRepoPresent}
                checked={chosen.has(`@${project.name}`)}
                onChange={() => toggleProject(project)}
              />
              {project.name}
            </label>
          ))}
        </div>
        <button type="button" className={styles.addPath} onClick={() => void addPath()}>
          Add path…
        </button>
        {submitted && membersProblem && <p className={styles.error}>{membersProblem}</p>}
      </fieldset>

      <Field label="Id" problem={submitted || idEdited || branch ? idProblem : null}>
        {(inputId) => (
          <input
            id={inputId}
            value={effectiveId}
            spellCheck={false}
            autoCapitalize="off"
            onChange={(e) => {
              setIdEdited(true);
              setId(e.target.value);
            }}
          />
        )}
      </Field>

      <fieldset className={styles.fieldset}>
        <legend>Branch</legend>
        <div className={styles.radios}>
          {(
            [
              ["auto", "Auto", "Check it out if it exists, else create it"],
              ["new", "New", "--new-branch"],
              ["existing", "Existing", "--existing-branch"],
            ] as const
          ).map(([value, label, title]) => (
            <label key={value} title={title}>
              <input
                type="radio"
                name="intent"
                value={value}
                checked={intent === value}
                onChange={() => setIntent(value)}
              />
              {label}
            </label>
          ))}
        </div>
      </fieldset>

      <details className={styles.advanced}>
        <summary>Advanced</summary>
        <label title={intent === "auto" ? "Needs New or Existing" : undefined}>
          <input
            type="checkbox"
            disabled={intent === "auto"}
            checked={intent !== "auto" && noFetch}
            onChange={(e) => setNoFetch(e.target.checked)}
          />
          Skip fetch
        </label>
        <label>
          <input
            type="checkbox"
            checked={noScripts}
            onChange={(e) => setNoScripts(e.target.checked)}
          />
          Skip post-create scripts
        </label>
      </details>

      {error && <p className={styles.error}>{error}</p>}
      <DialogActions>
        <button type="button" onClick={closeDialog}>
          Cancel
        </button>
        <button type="submit" className="primary" disabled={busy}>
          {failure?.retryable ? "Retry" : "Create plane"}
        </button>
      </DialogActions>
    </form>
  );
}

function FailureDetails({ failure }: { failure: CreateFailure }) {
  const hint =
    failure.envelope?.error === "fetch_failed"
      ? "git can't ask for credentials here. Set up an ssh-agent or a credential helper, then retry."
      : null;
  return (
    <div className={styles.failure} role="alert">
      <EnvelopeDetails envelope={failure.envelope} />
      {hint && <p className="muted">{hint}</p>}
      {failure.log && (
        <div className={styles.failureLog}>
          <Log text={failure.log} />
        </div>
      )}
    </div>
  );
}

function Field({
  label,
  problem,
  children,
}: {
  label: string;
  problem: string | null;
  children: (id: string) => React.ReactNode;
}) {
  const id = useId();
  return (
    <div className={styles.field}>
      <label className={styles.label} htmlFor={id}>
        {label}
      </label>
      {children(id)}
      {problem && <span className={styles.error}>{problem}</span>}
    </div>
  );
}

function TabButton({
  tab,
  current,
  onSelect,
  children,
}: {
  tab: Tab;
  current: Tab;
  onSelect: (tab: Tab) => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={tab === current}
      className={tab === current ? styles.activeTab : styles.tab}
      onClick={() => onSelect(tab)}
    >
      {children}
    </button>
  );
}

/** Every folder name in the planes directory, as far as the sidebar knows. */
function useExistingIds(): string[] {
  const rows = useStore((s) => s.planes.planes);
  return useMemo(() => rows.map((p) => p.id), [rows]);
}

type ProjectsState =
  | { kind: "loading" }
  | { kind: "loaded"; projects: Project[] }
  | { kind: "failed"; error: string };

function useProjects(): ProjectsState {
  const [state, setState] = useState<ProjectsState>({ kind: "loading" });
  useEffect(() => {
    let alive = true;
    commands.projectList().then(
      (projects) => alive && setState({ kind: "loaded", projects }),
      (error) => alive && setState({ kind: "failed", error: String(error) }),
    );
    return () => {
      alive = false;
    };
  }, []);
  return state;
}
