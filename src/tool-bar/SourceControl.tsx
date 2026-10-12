import {
  ArrowDown,
  ArrowUp,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  RefreshCw,
  TriangleAlert,
} from "lucide-react";
import { type ReactNode, useEffect } from "react";
import type { Change, ChangeKind, Repo, Status } from "../ipc";
import { labelFor } from "../keymap";
import {
  loadSourceControl,
  setAllReposCollapsed,
  toggleRepoCollapsed,
  useStore,
  watchSourceControl,
} from "../store/store";
import { Centered } from "../ui/Centered";
import { headLabel } from "./files/filesTree";
import { PanelActions, PanelButton } from "./PanelButton";
import styles from "./SourceControl.module.css";

export function SourceControl({ planeId }: { planeId: string }) {
  const sourceControl = useStore((s) => s.sourceControl);
  useEffect(() => {
    void loadSourceControl(planeId);
    return watchSourceControl(planeId);
  }, [planeId]);
  const current = sourceControl?.planeId === planeId ? sourceControl : null;
  if (current?.error) {
    return (
      <Centered title="Couldn't read git status">
        <p>{current.error}</p>
      </Centered>
    );
  }
  if (!current?.repos) return <Centered title="Reading git status…" />;
  if (current.repos.length === 0) {
    return (
      <Centered title="No repositories">
        <p>This plane has no Members and no git repos at its root or in projects/.</p>
      </Centered>
    );
  }
  return (
    <div className={styles.repos}>
      {current.repos.map((repo) => (
        <RepoSection key={repo.path} repo={repo} />
      ))}
    </div>
  );
}

export function SourceControlActions({ planeId }: { planeId: string }) {
  const platform = useStore((s) => s.platform);
  return (
    <PanelActions>
      <PanelButton title="Expand all" onClick={() => setAllReposCollapsed(false)}>
        <ChevronsUpDown size={14} strokeWidth={1.75} />
      </PanelButton>
      <PanelButton title="Collapse all" onClick={() => setAllReposCollapsed(true)}>
        <ChevronsDownUp size={14} strokeWidth={1.75} />
      </PanelButton>
      <PanelButton
        title={`Refresh (${labelFor(platform, "refresh")})`}
        onClick={() => void loadSourceControl(planeId)}
      >
        <RefreshCw size={14} strokeWidth={1.75} />
      </PanelButton>
    </PanelActions>
  );
}

const KIND_LETTERS: Record<ChangeKind, string> = {
  modified: "M",
  typeChanged: "T",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  conflicted: "!",
};

function RepoSection({ repo }: { repo: Repo }) {
  const expanded = useStore((s) => !s.repoCollapsed[repo.path]);
  const status = repo.state.kind === "ready" ? repo.state.status : null;
  return (
    <section className={styles.repo} aria-label={repo.name}>
      <button
        type="button"
        className={styles.repoHead}
        aria-expanded={expanded}
        title={repo.path}
        onClick={() => toggleRepoCollapsed(repo.path)}
      >
        <ChevronRight
          className={`${styles.chevron} ${expanded ? styles.open : ""}`}
          size={14}
          strokeWidth={1.75}
        />
        <span className={styles.repoName}>{repo.name}</span>
        {status && <RepoSummary status={status} />}
        {repo.state.kind !== "ready" && (
          <TriangleAlert className={styles.warn} size={13} aria-label="warning" />
        )}
      </button>
      {expanded && <RepoBody repo={repo} />}
    </section>
  );
}

function RepoSummary({ status }: { status: Status }) {
  const changes = status.staged.length + status.unstaged.length + status.untracked.length;
  const { ahead = 0, behind = 0 } = status.tracking ?? {};
  return (
    <>
      <span className={styles.branch}>{headLabel(status.head)}</span>
      {ahead > 0 && (
        <span className={styles.sync} title={`${ahead} ahead of upstream`}>
          <ArrowUp size={11} />
          {ahead}
        </span>
      )}
      {behind > 0 && (
        <span className={styles.sync} title={`${behind} behind upstream`}>
          <ArrowDown size={11} />
          {behind}
        </span>
      )}
      {changes > 0 && (
        <span className={styles.count} title={`${changes} changes`}>
          {changes}
        </span>
      )}
    </>
  );
}

function RepoBody({ repo }: { repo: Repo }) {
  switch (repo.state.kind) {
    case "missing":
      return <Row warn>Worktree missing: {repo.path}</Row>;
    case "failed":
      return <Row warn>{repo.state.message}</Row>;
    case "ready": {
      const { staged, unstaged, untracked } = repo.state.status;
      if (staged.length + unstaged.length + untracked.length === 0) {
        return <Row>No changes</Row>;
      }
      return (
        <>
          <Group title="Staged" changes={staged} />
          <Group title="Changes" changes={unstaged} />
          <Group
            title="Untracked"
            changes={untracked.map((path) => ({ path, kind: "added", from: null }))}
            letter="U"
          />
        </>
      );
    }
  }
}

function Group({ title, changes, letter }: { title: string; changes: Change[]; letter?: string }) {
  if (changes.length === 0) return null;
  return (
    <ul className={styles.group} aria-label={title}>
      <li className={styles.groupHead}>
        {title}
        <span className={styles.count}>{changes.length}</span>
      </li>
      {changes.map((change) => (
        <li
          key={change.path}
          className={styles.file}
          title={change.from ? `${change.from} → ${change.path}` : change.path}
        >
          <span className={styles.fileName}>{fileName(change.path)}</span>
          <span className={styles.fileDir}>{dirName(change.path)}</span>
          <span className={`${styles.kind} ${styles[letter ? "untracked" : change.kind]}`}>
            {letter ?? KIND_LETTERS[change.kind]}
          </span>
        </li>
      ))}
    </ul>
  );
}

function Row({ children, warn = false }: { children: ReactNode; warn?: boolean }) {
  return <p className={`${styles.row} ${warn ? styles.warnRow : ""}`}>{children}</p>;
}

function fileName(path: string): string {
  const trimmed = path.replace(/\/$/, "");
  return trimmed.slice(trimmed.lastIndexOf("/") + 1) + (path.endsWith("/") ? "/" : "");
}

function dirName(path: string): string {
  const trimmed = path.replace(/\/$/, "");
  return trimmed.slice(0, Math.max(trimmed.lastIndexOf("/"), 0));
}
