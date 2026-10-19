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
import { activeFileTab, findPlane } from "../store/selectors";
import {
  loadSourceControl,
  openFileTab,
  setAllReposCollapsed,
  toggleRepoCollapsed,
  useStore,
  watchSourceControl,
} from "../store/store";
import type { FileTabMode } from "../tabs/fileTabs";
import { Centered } from "../ui/Centered";
import { headLabel, repoPrefix } from "./files/filesTree";
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
        <RepoSection key={repo.path} planeId={planeId} repo={repo} />
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

function RepoSection({ planeId, repo }: { planeId: string; repo: Repo }) {
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
      {expanded && <RepoBody planeId={planeId} repo={repo} />}
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

function RepoBody({ planeId, repo }: { planeId: string; repo: Repo }) {
  const prefix = useStore((s) => repoPrefix(repo, findPlane(s, planeId)?.directory ?? ""));
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
      const target = prefix === null ? null : { planeId, prefix };
      return (
        <>
          <Group title="Staged" changes={staged} target={target} mode="index" />
          <Group title="Changes" changes={unstaged} target={target} mode="worktree" />
          <Group
            title="Untracked"
            changes={untracked.map((path) => ({ path, kind: "added", from: null }))}
            target={target}
            mode="file"
            letter="U"
          />
        </>
      );
    }
  }
}

/** Where a repo's change paths sit in the plane; `null` for a repo outside it. */
interface Target {
  planeId: string;
  /** The repo's folder in the plane: "" at the root, else "dir/". */
  prefix: string;
}

function Group({
  title,
  changes,
  target,
  mode,
  letter,
}: {
  title: string;
  changes: Change[];
  target: Target | null;
  mode: FileTabMode;
  letter?: string;
}) {
  const active = useStore((s) => (target ? activeFileTab(s, target.planeId) : undefined));
  if (changes.length === 0) return null;
  return (
    <ul className={styles.group} aria-label={title}>
      <li className={styles.groupHead}>
        {title}
        <span className={styles.count}>{changes.length}</span>
      </li>
      {changes.map((change) => {
        const row = (
          <>
            <span className={styles.fileName}>{fileName(change.path)}</span>
            <span className={styles.fileDir}>{dirName(change.path)}</span>
            <span className={`${styles.kind} ${styles[letter ? "untracked" : change.kind]}`}>
              {letter ?? KIND_LETTERS[change.kind]}
            </span>
          </>
        );
        const title = change.from ? `${change.from} → ${change.path}` : change.path;
        // An untracked folder (`dir/`) has no single file to open.
        if (!target || change.path.endsWith("/")) {
          return (
            <li key={change.path} className={styles.file} title={title}>
              {row}
            </li>
          );
        }
        const path = target.prefix + change.path;
        const current = active?.path === path && active.mode === mode;
        return (
          <li key={change.path}>
            <button
              type="button"
              className={`${styles.file} ${current ? styles.current : ""}`}
              title={title}
              aria-current={current || undefined}
              onClick={() => openFileTab(target.planeId, path, mode)}
            >
              {row}
            </button>
          </li>
        );
      })}
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
