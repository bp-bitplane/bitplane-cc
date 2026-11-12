import { useShallow } from "zustand/react/shallow";
import type { AgentInfo } from "../ipc";
import { resumableAgents } from "../store/selectors";
import { closeDialog, resumeAgent, type State, startNewSession, useStore } from "../store/store";
import { agentKindName, KindIcon } from "../tabs/KindIcon";
import { ListboxOption, useListbox } from "./Listbox";
import { Modal } from "./Modal";
import styles from "./Palette.module.css";

export type LauncherItem =
  | { key: string; type: "terminal"; label: string; disabled: false }
  | {
      key: string;
      type: "agent";
      kind: string;
      label: string;
      disabled: boolean;
      why: string | null;
    }
  | { key: string; type: "resume"; agent: AgentInfo; label: string; disabled: false };

/**
 * What can be started in a plane: a terminal, then each agent kind the
 * daemon knows (unavailable ones disabled, with why), then the plane's
 * agent sessions that can be resumed.
 */
export function launcherItems(
  state: Pick<State, "agentKinds" | "agents" | "planes">,
  planeId: string,
): LauncherItem[] {
  return [
    { key: "terminal", type: "terminal", label: "Terminal", disabled: false },
    ...state.agentKinds.map(
      (k): LauncherItem => ({
        key: `kind:${k.kind}`,
        type: "agent",
        kind: k.kind,
        label: agentKindName(k.kind),
        disabled: !k.available,
        why: k.available ? null : (k.error ?? "unavailable"),
      }),
    ),
    ...resumableAgents(state)
      .filter((r) => r.planeId === planeId)
      .map(
        ({ agent }): LauncherItem => ({
          key: `resume:${agent.agentSession}`,
          type: "resume",
          agent,
          label: `Resume ${agent.name ?? agent.agentSession}`,
          disabled: false,
        }),
      ),
  ];
}

/** "+", ⌘T and an empty plane: type to filter, ↑/↓ to move, Enter to start. */
export function Launcher({ planeId }: { planeId: string }) {
  const state = useStore(
    useShallow(({ agentKinds, agents, planes }) => ({ agentKinds, agents, planes })),
  );
  const items = launcherItems(state, planeId);
  const { matches, index, choose, input } = useListbox(
    (needle) => items.filter((item) => item.label.toLowerCase().includes(needle)),
    (item) => {
      if (item.type === "resume") void resumeAgent(item.agent, planeId);
      else void startNewSession(planeId, item.type === "agent" ? item.kind : null);
    },
  );

  return (
    <Modal label={`Start in ${planeId}`} onClose={closeDialog}>
      <input
        // biome-ignore lint/a11y/noAutofocus: the launcher exists to be typed into
        autoFocus
        className={styles.input}
        placeholder={`Start in ${planeId}…`}
        aria-label={`Start in ${planeId}`}
        {...input}
      />
      <div className={styles.list} role="listbox" aria-label="Start">
        {matches.map((item, i) => (
          <ListboxOption
            key={item.key}
            selected={i === index}
            disabled={item.disabled}
            title={item.type === "agent" && item.why ? item.why : undefined}
            onPick={() => choose(item)}
          >
            <span className={styles.item}>
              <KindIcon kind={iconKind(item)} />
              <span>{item.label}</span>
            </span>
            {item.type === "agent" && item.why && <span className={styles.why}>{item.why}</span>}
          </ListboxOption>
        ))}
        {matches.length === 0 && <div className={`${styles.row} muted`}>Nothing matches</div>}
      </div>
    </Modal>
  );
}

function iconKind(item: LauncherItem): string | null {
  switch (item.type) {
    case "terminal":
      return null;
    case "agent":
      return item.kind;
    case "resume":
      return item.agent.kind;
  }
}
