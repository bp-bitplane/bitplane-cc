import { useEffect, useState } from "react";
import type { AgentInfo, SessionInfo } from "../ipc";
import type { Platform } from "../keymap";
import { agentOf, agentStateLabel } from "../store/selectors";
import { useStore } from "../store/store";
import { KindIcon } from "./KindIcon";
import styles from "./SessionSlot.module.css";

/**
 * A session's kind icon with its agent's state as a badge, or its tab number
 * while the go-to-tab modifier is held; the slot keeps its width either way.
 */
export function SessionSlot({
  session,
  position,
  numbered,
  className,
}: {
  session: SessionInfo;
  position: number;
  numbered: boolean;
  className?: string;
}) {
  const agent = useStore((s) => agentOf(s, session));
  return (
    <span className={className ? `${styles.slot} ${className}` : styles.slot}>
      {numbered ? (
        <span className={styles.number}>{position}</span>
      ) : (
        <KindIcon kind={session.agent} />
      )}
      {agent && <AgentBadge agent={agent} />}
    </span>
  );
}

/** Whether the modifier of the go-to-tab shortcut (⌘ on macOS) is held down. */
export function useNumbersShown(platform: Platform): boolean {
  const [held, setHeld] = useState(false);
  useEffect(() => {
    const update = (e: KeyboardEvent) =>
      setHeld(platform === "mac" ? e.metaKey : e.ctrlKey && e.altKey);
    const release = () => setHeld(false);
    window.addEventListener("keydown", update, true);
    window.addEventListener("keyup", update, true);
    window.addEventListener("blur", release);
    return () => {
      window.removeEventListener("keydown", update, true);
      window.removeEventListener("keyup", update, true);
      window.removeEventListener("blur", release);
    };
  }, [platform]);
  return held;
}

function AgentBadge({ agent }: { agent: AgentInfo }) {
  const state = agentStateLabel(agent);
  if (state === null) return null;
  const label = `Agent ${state}`;
  return (
    <span
      className={styles.agent}
      data-state={agent.state.kind}
      role="img"
      aria-label={label}
      title={label}
    />
  );
}
