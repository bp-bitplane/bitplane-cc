import { useState } from "react";
import { findPlane, resumableAgents, sessionsOf } from "../store/selectors";
import {
  closeDialog,
  openClaudeTerminal,
  resumeAgent,
  selectPlane,
  useStore,
} from "../store/store";
import { PlaneBadges } from "../ui/Badges";
import { Modal } from "./Modal";
import styles from "./Palette.module.css";

/** ⌘P: filter plane ids and actions by substring, ↑/↓ to move, Enter to pick. */
export function Palette() {
  const planes = useStore((s) => s.planes);
  const rows = planes.planes;
  const sessions = useStore((s) => s.sessions);
  const agents = useStore((s) => s.agents);
  const selected = useStore((s) => {
    const plane = s.selectedPlaneId ? findPlane(s, s.selectedPlaneId) : undefined;
    return plane?.kind === "plane" || plane?.kind === "draft" ? plane.id : null;
  });
  const resumable = resumableAgents({ agents, planes });
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const needle = query.trim().toLowerCase();
  const matches = rows.filter((p) => p.id.includes(needle));
  // A stopgap until the launcher palette (bp-bitplane/bitplane-cc#66).
  const newClaude = selected
    ? [
        {
          key: "new-claude",
          label: `New Claude terminal in ${selected}`,
          run: () => openClaudeTerminal(selected),
        },
      ]
    : [];
  const actions = [
    ...newClaude,
    ...resumable.map(({ agent, planeId }) => ({
      key: agent.agentSession,
      label: `Resume agent session ${agent.name ?? agent.agentSession} in ${planeId}`,
      run: () => resumeAgent(agent, planeId),
    })),
  ].filter((a) => a.label.toLowerCase().includes(needle));
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
              key={action.key}
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
