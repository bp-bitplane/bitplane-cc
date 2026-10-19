import { File, Plus, X } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { labelFor } from "../keymap";
import { activeTabId, fileTabsOf, findPlane, sessionsOf, tabLabel } from "../store/selectors";
import {
  closeFileTab,
  openSession,
  pinFileTab,
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
      {sessions.map((session, index) => {
        const label = tabLabel(session, index + 1);
        const active = session.id === current;
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
