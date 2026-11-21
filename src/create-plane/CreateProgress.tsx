import { useEffect, useRef } from "react";
import type { ErrorEnvelope, Phase } from "../ipc";
import { cancelCreate, dismissCreate, useStore } from "../store/store";
import { Centered } from "../ui/Centered";
import styles from "./CreateProgress.module.css";

const PHASES: { phase: Phase; label: string }[] = [
  { phase: "fetching", label: "Fetching" },
  { phase: "addingWorktrees", label: "Adding worktrees" },
  { phase: "runningScripts", label: "Running scripts" },
];

/**
 * A plane being created: live log, phase, Cancel; or how it ended, when its
 * plane has no row of its own (a partial plane, or one never made).
 */
export function CreateProgress({ planeId }: { planeId: string }) {
  const create = useStore((s) => s.creates[planeId]);
  if (!create) return <Centered title={`Creating ${planeId}…`} />;
  const startedBy = <p className={styles.startedBy}>Started by {create.startedBy}</p>;
  const dismiss = (
    <div className={styles.actions}>
      <button type="button" onClick={() => void dismissCreate(planeId)}>
        Dismiss
      </button>
    </div>
  );

  switch (create.status.kind) {
    case "partial":
      return (
        <div className={styles.progress}>
          <h2>{planeId} was left half-created</h2>
          <EnvelopeDetails envelope={create.status.envelope} />
          <p>
            The app never destroys planes. Remove what's left with <code>bp destroy {planeId}</code>
            , then create it again.
          </p>
          <Log text={create.log} />
          {dismiss}
        </div>
      );
    case "nothingCreated":
      return (
        <div className={styles.progress}>
          <h2>{planeId} wasn't created</h2>
          <EnvelopeDetails envelope={create.status.envelope} />
          {startedBy}
          <Log text={create.log} />
          {dismiss}
        </div>
      );
    case "failed":
      return (
        <div className={styles.progress}>
          <h2>Creating {planeId} failed</h2>
          <p>
            bp gave no answer ({create.status.reason}). Check the plane with <code>bp list</code>{" "}
            before creating it again.
          </p>
          {startedBy}
          <Log text={create.log} />
          {dismiss}
        </div>
      );
    default:
      break;
  }

  const running = create.status.kind === "running";
  const current = PHASES.findIndex((p) => p.phase === create.phase);
  return (
    <div className={styles.progress}>
      <h2>Creating {planeId}…</h2>
      <ol className={styles.phases} aria-label="Phase">
        {PHASES.map((p, index) => (
          <li
            key={p.phase}
            className={index === current ? styles.current : index < current ? styles.done : ""}
            aria-current={index === current ? "step" : undefined}
          >
            {p.label}
          </li>
        ))}
      </ol>
      {startedBy}
      <Log text={create.log} />
      {running ? (
        <div className={styles.actions}>
          <button type="button" onClick={() => void cancelCreate(planeId)}>
            Cancel
          </button>
        </div>
      ) : (
        dismiss
      )}
    </div>
  );
}

export function EnvelopeDetails({ envelope }: { envelope: ErrorEnvelope | null }) {
  if (!envelope) return null;
  return (
    <div className={styles.envelope}>
      <p>{envelope.message}</p>
      {envelope.problems.length > 0 && (
        <ul>
          {envelope.problems.map((problem, index) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: problems have no identity of their own
            <li key={index}>
              {problem.subject && <strong>{problem.subject}: </strong>}
              {problem.message}
            </li>
          ))}
        </ul>
      )}
      {envelope.remedy && <p className={styles.remedy}>{envelope.remedy}</p>}
    </div>
  );
}

/** A monospace log that follows its tail. */
export function Log({ text }: { text: string }) {
  const ref = useRef<HTMLPreElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll whenever the text grows
  useEffect(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [text]);
  return (
    <pre ref={ref} className={styles.log} role="log" aria-label="Log">
      {text || " "}
    </pre>
  );
}
