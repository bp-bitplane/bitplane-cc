// PROTOTYPE (alfonz/proto-git-graph): a repo's history drawn with @bitplane/git-graph.
import { createGraph, type GraphNode } from "@bitplane/git-graph";
import { HEIGHT, renderRails } from "@bitplane/git-graph/react";
import "@bitplane/git-graph/react/git-graph.css";
import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { commands, type GitLog, type LogCommit } from "../ipc";
import type { GraphTab } from "../tabs/graphTabs";
import { Centered } from "../ui/Centered";
import styles from "./GraphTabView.module.css";

const RAIL_WIDTH = 16;

export function GraphTabView({ tab }: { tab: GraphTab }) {
  const [log, setLog] = useState<GitLog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    setError(null);
    commands
      .protoPlaneGitLog(tab.planeId, tab.repoPath)
      .then(setLog)
      .catch((e) => setError(String(e)));
  }, [tab.planeId, tab.repoPath]);
  useEffect(load, [load]);

  const nodes = useMemo(() => {
    if (!log) return [];
    const graph = createGraph<LogCommit>();
    graph.append(log.commits);
    graph.end();
    return graph.nodes as readonly GraphNode<LogCommit>[];
  }, [log]);
  const railsWidth = Math.max(
    0,
    ...nodes.map((n) => (n.rails.length + n.merges.length + 2) * RAIL_WIDTH),
  );

  return (
    <div className={styles.view}>
      <div className={styles.header}>
        <span className={styles.name}>{tab.name}</span>
        <span className={styles.path}>{tab.repoPath}</span>
        {log && (
          <span className={styles.count}>
            {log.commits.length}
            {log.truncated ? ` of ≥${log.commits.length}` : ""} commits
          </span>
        )}
        <button type="button" className={styles.refresh} title="Refresh" onClick={load}>
          <RefreshCw size={13} />
        </button>
      </div>
      {error ? (
        <Centered title="Couldn't read the history">
          <p>{error}</p>
        </Centered>
      ) : !log ? (
        <Centered title="Reading history…" />
      ) : (
        <ol className={styles.rows}>
          {nodes.map((node) => {
            const commit = node.commit as LogCommit;
            const head = commit.refs.some((r) => r === "HEAD" || r.startsWith("HEAD -> "));
            return (
              <li
                key={commit.hash}
                className={`${styles.row} ${head ? styles.head : ""}`}
                style={{ height: HEIGHT }}
                title={commit.hash}
              >
                <span className={styles.rails} style={{ width: railsWidth }}>
                  {renderRails(node)}
                </span>
                <span className={styles.subject}>
                  {commit.refs.map((ref) => (
                    <RefBadge key={ref} gitRef={ref} />
                  ))}
                  {commit.subject}
                </span>
                <span className={styles.author}>{commit.author}</span>
                <span className={styles.date} title={new Date(commit.time * 1000).toLocaleString()}>
                  {relative(commit.time)}
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}

function RefBadge({ gitRef }: { gitRef: string }) {
  if (gitRef.startsWith("HEAD -> ")) {
    return <span className={`${styles.ref} ${styles.refHead}`}>{gitRef.slice(8)}</span>;
  }
  if (gitRef === "HEAD") return <span className={`${styles.ref} ${styles.refHead}`}>HEAD</span>;
  if (gitRef.startsWith("tag: ")) {
    return <span className={`${styles.ref} ${styles.refTag}`}>{gitRef.slice(5)}</span>;
  }
  // `%D` can't tell a remote branch from a local one with a slash; the remote-name prefix is a guess.
  const remote = /^(origin|upstream)\//.test(gitRef) || gitRef.endsWith("/HEAD");
  return <span className={`${styles.ref} ${remote ? styles.refRemote : ""}`}>{gitRef}</span>;
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 31536000],
  ["month", 2592000],
  ["week", 604800],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
];
const format = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

function relative(time: number): string {
  const delta = time - Date.now() / 1000;
  for (const [unit, seconds] of UNITS) {
    if (Math.abs(delta) >= seconds) return format.format(Math.round(delta / seconds), unit);
  }
  return "just now";
}
