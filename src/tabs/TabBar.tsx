import { Plus, X } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { labelFor } from "../keymap";
import { activeSession, findPlane, sessionsOf, tabLabel } from "../store/selectors";
import { openSession, requestCloseSession, selectSession, useStore } from "../store/store";
import styles from "./TabBar.module.css";

/** The selected plane's sessions, numbered by position. */
export function TabBar({ planeId }: { planeId: string }) {
  const platform = useStore((s) => s.platform);
  const sessions = useStore(useShallow((s) => sessionsOf(s, planeId)));
  const current = useStore((s) => activeSession(s, planeId));
  const kind = useStore((s) => findPlane(s, planeId)?.kind);
  const canOpen = kind !== "gone" && kind !== "creating";

  return (
    <div className={styles.tabbar} role="tablist" aria-label={`Sessions in ${planeId}`}>
      {sessions.map((session, index) => {
        const label = tabLabel(session, index + 1);
        const active = session.id === current?.id;
        return (
          <div key={session.id} className={`${styles.tab} ${active ? styles.active : ""}`}>
            <button
              type="button"
              role="tab"
              aria-selected={active}
              className={styles.label}
              title={label}
              tabIndex={-1}
              onClick={() => selectSession(planeId, session.id)}
            >
              {label}
            </button>
            <button
              type="button"
              className={styles.close}
              aria-label={`Close ${label}`}
              tabIndex={-1}
              onClick={() => void requestCloseSession(session.id)}
            >
              <X size={13} />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        className={styles.new}
        disabled={!canOpen}
        title={`New session (${labelFor(platform, "newTab")})`}
        aria-label="New session"
        tabIndex={-1}
        onClick={() => void openSession(planeId)}
      >
        <Plus size={15} />
      </button>
    </div>
  );
}
