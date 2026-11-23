import type { PlaneRow } from "../ipc";
import { resumableAgents, sessionsOf } from "../store/selectors";
import { closeDialog, resumeAgent, selectPlane, useStore } from "../store/store";
import { PlaneBadges } from "../ui/Badges";
import { ListboxOption, useListbox } from "./Listbox";
import { Modal } from "./Modal";
import styles from "./Palette.module.css";

/** ⌘P: filter plane ids and actions by substring, ↑/↓ to move, Enter to pick. */
export function Palette() {
  const planes = useStore((s) => s.planes);
  const sessions = useStore((s) => s.sessions);
  const agents = useStore((s) => s.agents);
  const actions: Action[] = resumableAgents({ agents, planes }).map(({ agent, planeId }) => ({
    key: agent.agentSession,
    label: `Resume agent session ${agent.name ?? agent.agentSession} in ${planeId}`,
    run: () => resumeAgent(agent, planeId),
  }));
  const { matches, index, choose, input } = useListbox<Match>(
    (needle) => [
      ...planes.planes
        .filter((p) => p.id.includes(needle))
        .map((plane): Match => ({ type: "plane", plane })),
      ...actions
        .filter((a) => a.label.toLowerCase().includes(needle))
        .map((action): Match => ({ type: "action", action })),
    ],
    (match) => {
      if (match.type === "plane") {
        closeDialog();
        selectPlane(match.plane.id);
      } else {
        void match.action.run();
      }
    },
  );
  const planeCount = matches.filter((m) => m.type === "plane").length;
  const option = (match: Match, i: number) => (
    <ListboxOption
      key={match.type === "plane" ? match.plane.id : match.action.key}
      selected={i === index}
      onPick={() => choose(match)}
    >
      {match.type === "plane" ? (
        <>
          <span>{match.plane.id}</span>
          <PlaneBadges
            plane={match.plane}
            sessionCount={sessionsOf({ sessions }, match.plane.id).length}
          />
        </>
      ) : (
        <span>{match.action.label}</span>
      )}
    </ListboxOption>
  );

  return (
    <Modal label="Switch to plane" onClose={closeDialog}>
      <input
        // biome-ignore lint/a11y/noAutofocus: the palette exists to be typed into
        autoFocus
        className={styles.input}
        placeholder="Switch to plane…"
        aria-label="Switch to plane"
        {...input}
      />
      <div className={styles.list} role="listbox" aria-label="Planes">
        {matches.slice(0, planeCount).map(option)}
        {planeCount === 0 && <div className={`${styles.row} muted`}>No plane matches</div>}
        {matches.slice(planeCount).map((match, j) => option(match, planeCount + j))}
      </div>
    </Modal>
  );
}

interface Action {
  key: string;
  label: string;
  run: () => Promise<void>;
}

type Match = { type: "plane"; plane: PlaneRow } | { type: "action"; action: Action };
