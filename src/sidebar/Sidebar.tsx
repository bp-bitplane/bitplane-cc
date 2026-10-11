import { ChevronRight, ChevronsDownUp, ChevronsUpDown, Plus, RefreshCw, X } from "lucide-react";
import type { ReactNode } from "react";
import { useShallow } from "zustand/react/shallow";
import type { PlaneRow } from "../ipc";
import { labelFor } from "../keymap";
import { activeSession, isExpanded, sessionsOf, tabLabel } from "../store/selectors";
import {
  openCreateDialog,
  refresh,
  requestCloseSession,
  selectPlane,
  selectSession,
  setAllCollapsed,
  toggleCollapsed,
  useStore,
} from "../store/store";
import { PlaneBadges } from "../ui/Badges";
import styles from "./Sidebar.module.css";

export function Sidebar() {
  const platform = useStore((s) => s.platform);
  const rows = useStore((s) => s.planes.planes);
  return (
    <aside className={styles.sidebar}>
      <header className={styles.head} data-tauri-drag-region>
        {platform !== "mac" && <span>Planes</span>}
        <span className={styles.headActions}>
          <IconButton title="Expand all" onClick={() => setAllCollapsed(false)}>
            <ChevronsUpDown size={14} strokeWidth={1.75} />
          </IconButton>
          <IconButton title="Collapse all" onClick={() => setAllCollapsed(true)}>
            <ChevronsDownUp size={14} strokeWidth={1.75} />
          </IconButton>
          <IconButton
            title={`Refresh (${labelFor(platform, "refresh")})`}
            onClick={() => void refresh()}
          >
            <RefreshCw size={14} strokeWidth={1.75} />
          </IconButton>
          <IconButton
            title={`Create plane (${labelFor(platform, "createPlane")})`}
            onClick={openCreateDialog}
          >
            <Plus size={14} strokeWidth={1.75} />
          </IconButton>
        </span>
      </header>
      <nav className={styles.list} aria-label="Planes">
        {rows.map((plane) => (
          <PlaneGroup key={plane.id} plane={plane} />
        ))}
      </nav>
      <Footer />
    </aside>
  );
}

function PlaneGroup({ plane }: { plane: PlaneRow }) {
  const selected = useStore((s) => s.selectedPlaneId === plane.id);
  const sessions = useStore(useShallow((s) => sessionsOf(s, plane.id)));
  const expanded = useStore((s) => isExpanded(s, plane.id));
  const current = useStore((s) => activeSession(s, plane.id));
  const rowClass = [
    styles.plane,
    selected && styles.selected,
    plane.kind === "gone" && styles.gone,
    plane.kind === "draft" && styles.draft,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={`${styles.group} ${selected ? styles.groupSelected : ""}`}>
      <div className={rowClass}>
        <button
          type="button"
          className={`${styles.chevron} ${expanded ? styles.open : ""}`}
          style={{ visibility: sessions.length > 0 ? "visible" : "hidden" }}
          aria-label={expanded ? `Collapse ${plane.id}` : `Expand ${plane.id}`}
          aria-expanded={expanded}
          tabIndex={-1}
          onClick={() => toggleCollapsed(plane.id, expanded)}
        >
          <ChevronRight size={14} strokeWidth={1.75} />
        </button>
        <button
          type="button"
          className={styles.planeButton}
          aria-current={selected || undefined}
          tabIndex={-1}
          onClick={() => selectPlane(plane.id)}
        >
          <span className={styles.planeId}>{plane.id}</span>
          <PlaneBadges plane={plane} sessionCount={expanded ? 0 : sessions.length} />
        </button>
      </div>
      {expanded && (
        <div className={styles.sessions}>
          {sessions.map((session, index) => (
            <div
              key={session.id}
              className={`${styles.session} ${selected && session.id === current?.id ? styles.current : ""}`}
            >
              <button
                type="button"
                className={styles.sessionButton}
                tabIndex={-1}
                onClick={() => selectSession(plane.id, session.id)}
              >
                {tabLabel(session, index + 1)}
              </button>
              <button
                type="button"
                className={styles.close}
                aria-label={`Close ${tabLabel(session, index + 1)}`}
                tabIndex={-1}
                onClick={() => void requestCloseSession(session.id)}
              >
                <X size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Footer() {
  const planesDir = useStore((s) => s.planes.planesDir);
  const refreshedAt = useStore((s) => s.planes.refreshedAt);
  const envWarning = useStore((s) => s.planes.envWarning);
  return (
    <footer className={styles.footer}>
      {envWarning && <div className={styles.envWarning}>{envWarning}</div>}
      <div title="Resolved planes directory">
        {planesDir ?? "…"}
        {refreshedAt !== null && <> · refreshed {new Date(refreshedAt).toLocaleTimeString()}</>}
      </div>
    </footer>
  );
}

function IconButton({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className={styles.iconButton}
      title={title}
      aria-label={title}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
