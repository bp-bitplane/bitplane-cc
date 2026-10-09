import { useState } from "react";
import { sessionsOf } from "../store/selectors";
import { closeDialog, openAgentResume, selectPlane, useStore } from "../store/store";
import { PlaneBadges } from "../ui/Badges";
import { Modal } from "./Modal";
import styles from "./Palette.module.css";

const ACTIONS = [{ label: "Enable agent resume", run: openAgentResume }];

/** ⌘P: filter plane ids and actions by substring, ↑/↓ to move, Enter to pick. */
export function Palette() {
  const rows = useStore((s) => s.planes.planes);
  const sessions = useStore((s) => s.sessions);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const needle = query.trim().toLowerCase();
  const matches = rows.filter((p) => p.id.includes(needle));
  const actions = ACTIONS.filter((a) => a.label.toLowerCase().includes(needle));
  const total = matches.length + actions.length;
  const pick = (i: number) => {
    const plane = matches[i];
    const action = actions[i - matches.length];
    if (plane) {
      closeDialog();
      selectPlane(plane.id);
    } else if (action) {
      void action.run();
    }
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
            setIndex((i) => Math.min(i + 1, total - 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setIndex((i) => Math.max(i - 1, 0));
          } else if (e.key === "Enter") {
            pick(index);
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
            onClick={() => pick(i)}
            onKeyDown={undefined}
          >
            <span>{plane.id}</span>
            <PlaneBadges plane={plane} sessionCount={sessionsOf({ sessions }, plane.id).length} />
          </div>
        ))}
        {matches.length === 0 && <div className={`${styles.row} muted`}>No plane matches</div>}
        {actions.map((action, j) => {
          const i = matches.length + j;
          return (
            <div
              key={action.label}
              role="option"
              tabIndex={-1}
              aria-selected={i === index}
              className={`${styles.row} ${i === index ? styles.active : ""}`}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => pick(i)}
              onKeyDown={undefined}
            >
              <span>{action.label}</span>
            </div>
          );
        })}
      </div>
    </Modal>
  );
}
