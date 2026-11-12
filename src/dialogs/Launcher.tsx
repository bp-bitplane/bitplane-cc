import { useState } from "react";
import { useShallow } from "zustand/react/shallow";
import type { AgentInfo } from "../ipc";
import { agentKindName, resumableAgents } from "../store/selectors";
import { closeDialog, launch, resumeAgent, type State, useStore } from "../store/store";
import { KindIcon } from "../tabs/KindIcon";
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
  const mirror = useStore(
    useShallow(({ agentKinds, agents, planes }) => ({ agentKinds, agents, planes })),
  );
  const items = launcherItems(mirror, planeId);
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const matches = items.filter((item) => item.label.toLowerCase().includes(needle));
  const [index, setIndex] = useState(() => step(matches, -1, 1));
  const pick = (item: LauncherItem | undefined) => {
    if (!item || item.disabled) return;
    if (item.type === "resume") void resumeAgent(item.agent, planeId);
    else void launch(planeId, item.type === "agent" ? item.kind : null);
  };

  return (
    <Modal label={`Start in ${planeId}`} onClose={closeDialog}>
      <input
        // biome-ignore lint/a11y/noAutofocus: the launcher exists to be typed into
        autoFocus
        className={styles.input}
        placeholder={`Start in ${planeId}…`}
        aria-label={`Start in ${planeId}`}
        value={query}
        onChange={(e) => {
          const next = e.target.value;
          setQuery(next);
          const filtered = items.filter((item) =>
            item.label.toLowerCase().includes(next.trim().toLowerCase()),
          );
          setIndex(step(filtered, -1, 1));
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setIndex((i) => step(matches, i, 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setIndex((i) => step(matches, i, -1));
          } else if (e.key === "Enter") {
            pick(matches[index]);
          }
        }}
      />
      <div className={styles.list} role="listbox" aria-label="Start">
        {matches.map((item, i) => (
          <div
            key={item.key}
            role="option"
            tabIndex={-1}
            aria-selected={i === index}
            aria-disabled={item.disabled}
            title={item.type === "agent" && item.why ? item.why : undefined}
            className={`${styles.row} ${i === index ? styles.active : ""} ${item.disabled ? styles.disabled : ""}`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => pick(item)}
            onKeyDown={undefined}
          >
            <span className={styles.item}>
              <KindIcon kind={iconKind(item)} />
              <span>{item.label}</span>
            </span>
            {item.type === "agent" && item.why && <span className={styles.why}>{item.why}</span>}
          </div>
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

/** The next enabled item from `from` in `delta`'s direction; `from` itself when there's none. */
function step(items: LauncherItem[], from: number, delta: 1 | -1): number {
  for (let i = from + delta; i >= 0 && i < items.length; i += delta) {
    if (!items[i]?.disabled) return i;
  }
  return Math.max(from, 0);
}
