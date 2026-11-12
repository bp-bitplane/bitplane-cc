import { useEffect, useRef } from "react";
import type { SessionInfo } from "../ipc";
import { findPlane, hasAgentLink } from "../store/selectors";
import { closeSession, restartSession, useStore } from "../store/store";
import { agentKindName } from "../tabs/KindIcon";
import styles from "./AgentOverlay.module.css";

/**
 * Over an agent terminal's frozen screen once its agent exited or couldn't
 * start. OK closes the tab; the other button relaunches it, resuming the
 * agent session when the tab has one.
 */
export function AgentOverlay({ session, visible }: { session: SessionInfo; visible: boolean }) {
  const linked = useStore((s) => hasAgentLink(s, session));
  const planeGone = useStore((s) => findPlane(s, session.planeId)?.kind === "gone");
  const ok = useRef<HTMLButtonElement>(null);
  const { status } = session;
  useEffect(() => {
    if (visible && status.kind !== "running") ok.current?.focus();
  }, [visible, status.kind]);

  if (status.kind === "running") return null;
  const name = agentKindName(session.agent ?? "");
  const failed = status.kind === "spawnFailed";
  const title = failed ? `${name} couldn't start` : `${name} exited`;
  const code = status.kind === "exited" && status.code !== 0 ? ` (code ${status.code})` : "";
  const gone = planeGone || session.rootGone;
  const close = () => void closeSession(session.id);
  return (
    <div
      className={styles.overlay}
      role="alertdialog"
      aria-label={title}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          close();
        }
      }}
    >
      <div className={styles.card}>
        <p className={styles.title}>
          {title}
          {code}
        </p>
        {failed && <p className={styles.error}>{status.error}</p>}
        <div className={styles.actions}>
          <button
            type="button"
            disabled={gone}
            title={gone ? "The plane is gone" : undefined}
            onClick={() => void restartSession(session.id)}
          >
            {failed ? "Retry" : linked ? "Resume" : "Start again"}
          </button>
          <button ref={ok} type="button" className="primary" onClick={close}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/** Whether the session shows the overlay instead of taking keystrokes. */
export function agentOverlaid(session: SessionInfo): boolean {
  return session.agent !== null && session.status.kind !== "running";
}
