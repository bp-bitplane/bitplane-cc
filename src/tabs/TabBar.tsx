import { File, Plus, X } from "lucide-react";
import { useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import type { SessionInfo } from "../ipc";
import { labelFor } from "../keymap";
import {
  activeTabId,
  fileTabsOf,
  findPlane,
  programTitle,
  sessionsOf,
  tabLabel,
} from "../store/selectors";
import {
  closeFileTab,
  openSession,
  pinFileTab,
  renameSession,
  requestCloseSession,
  selectSession,
  selectTab,
  useStore,
} from "../store/store";
import { fileTabKey, fileTabTitle, folderOf, folderSuffixes } from "./fileTabs";
import styles from "./TabBar.module.css";

/** The selected plane's Session tabs, numbered by position, then its File tabs. */
export function TabBar({ planeId }: { planeId: string }) {
  const platform = useStore((s) => s.platform);
  const sessions = useStore(useShallow((s) => sessionsOf(s, planeId)));
  const fileTabs = useStore((s) => fileTabsOf(s, planeId));
  const current = useStore((s) => activeTabId(s, planeId));
  const kind = useStore((s) => findPlane(s, planeId)?.kind);
  const canOpen = kind !== "gone" && kind !== "creating";
  const suffixes = folderSuffixes(fileTabs);

  return (
    <div
      className={styles.tabbar}
      role="tablist"
      aria-label={`Tabs in ${planeId}`}
      data-tauri-drag-region
    >
      {sessions.map((session, index) => (
        <SessionTab
          key={session.id}
          session={session}
          position={index + 1}
          active={session.id === current}
        />
      ))}
      {fileTabs.map((tab) => {
        const key = fileTabKey(tab);
        const title = fileTabTitle(tab);
        const suffix = suffixes.get(key);
        const folder = folderOf(tab.path);
        const active = key === current;
        return (
          <div
            key={key}
            className={`${styles.tab} ${active ? styles.active : ""} ${tab.pinned ? "" : styles.preview}`}
          >
            <button
              type="button"
              role="tab"
              aria-selected={active}
              className={styles.label}
              title={folder ? `${planeId}/${folder}` : planeId}
              tabIndex={-1}
              onClick={() => selectTab(planeId, key)}
              onDoubleClick={() => pinFileTab(planeId, key)}
            >
              <File size={13} className={styles.icon} aria-hidden />
              <span className={styles.name}>{title}</span>
              {suffix && <span className={styles.suffix}>{suffix}</span>}
            </button>
            <button
              type="button"
              className={styles.close}
              aria-label={`Close ${title}`}
              tabIndex={-1}
              onClick={() => closeFileTab(planeId, key)}
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

/** A Session tab; double-clicking its label renames it (`session.rename`). */
function SessionTab({
  session,
  position,
  active,
}: {
  session: SessionInfo;
  position: number;
  active: boolean;
}) {
  const [editing, setEditing] = useState(false);
  // An input removed while focused may still blur; only the first finish counts.
  const finished = useRef(false);
  const label = tabLabel(session, position);
  const title = programTitle(session);
  const finish = (name: string | null) => {
    if (finished.current) return;
    finished.current = true;
    setEditing(false);
    if (name !== null && name !== (session.name ?? "")) void renameSession(session.id, name);
  };

  return (
    <div className={`${styles.tab} ${active ? styles.active : ""}`}>
      {editing ? (
        <input
          className={styles.rename}
          aria-label={`Rename ${label}`}
          defaultValue={session.name ?? ""}
          placeholder={session.title ?? session.shellName}
          // biome-ignore lint/a11y/noAutofocus: the field appears because the user asked to type in it
          autoFocus
          onKeyDown={(e) => {
            if (e.key === "Enter") finish(e.currentTarget.value);
            else if (e.key === "Escape") finish(null);
          }}
          onBlur={(e) => finish(e.currentTarget.value)}
        />
      ) : (
        <button
          type="button"
          role="tab"
          aria-selected={active}
          aria-label={label}
          className={styles.label}
          title={title ? `${label} — ${title}` : label}
          tabIndex={-1}
          onClick={() => selectSession(session.planeId, session.id)}
          onDoubleClick={() => {
            finished.current = false;
            setEditing(true);
          }}
        >
          {label}
          {title && <span className={styles.suffix}>{title}</span>}
        </button>
      )}
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
}
