import { useState } from "react";
import { sessionsOf } from "../store/selectors";
import { closeDialog, selectPlane, useStore } from "../store/store";
import { PlaneBadges } from "../ui/Badges";
import { Modal } from "./Modal";
import styles from "./Palette.module.css";

/** ⌘P: filter plane ids by substring, ↑/↓ to move, Enter to select. */
export function Palette() {
  const rows = useStore((s) => s.planes.planes);
  const sessions = useStore((s) => s.sessions);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const matches = rows.filter((p) => p.id.includes(query.trim().toLowerCase()));
  const pick = (planeId: string | undefined) => {
    if (!planeId) return;
    closeDialog();
    selectPlane(planeId);
  };

  return (
    <Modal label="Switch to plane" onClose={closeDialog}>
      <input
        // biome-ignore lint/a11y/noAutofocus: the palette exists to be typed into
        autoFocus
        className={styles.input}
        placeholder="Switch to plane…"
        aria-label="Switch to plane"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setIndex(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setIndex((i) => Math.min(i + 1, matches.length - 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setIndex((i) => Math.max(i - 1, 0));
          } else if (e.key === "Enter") {
            pick(matches[index]?.id);
          }
        }}
      />
      <div className={styles.list} role="listbox" aria-label="Planes">
        {matches.map((plane, i) => (
          <div
            key={plane.id}
            role="option"
            tabIndex={-1}
            aria-selected={i === index}
            className={`${styles.row} ${i === index ? styles.active : ""}`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => pick(plane.id)}
            onKeyDown={undefined}
          >
            <span>{plane.id}</span>
            <PlaneBadges plane={plane} sessionCount={sessionsOf({ sessions }, plane.id).length} />
          </div>
        ))}
        {matches.length === 0 && <div className={`${styles.row} muted`}>No plane matches</div>}
      </div>
    </Modal>
  );
}
