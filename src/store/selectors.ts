import type { PlaneRow, SessionInfo } from "../ipc";
import type { State } from "./store";

type Mirror = Pick<State, "planes" | "sessions">;

export function findPlane(state: Mirror, planeId: string): PlaneRow | undefined {
  return state.planes.planes.find((p) => p.id === planeId);
}

/** A plane's tabs, in creation order. */
export function sessionsOf(state: Pick<State, "sessions">, planeId: string): SessionInfo[] {
  return Object.values(state.sessions)
    .filter((s) => s.planeId === planeId)
    .sort((a, b) => a.seq - b.seq);
}

export function activeSession(
  state: Pick<State, "sessions" | "activeTab">,
  planeId: string | null,
): SessionInfo | undefined {
  if (!planeId) return undefined;
  const tabs = sessionsOf(state, planeId);
  return tabs.find((s) => s.id === state.activeTab[planeId]) ?? tabs[0];
}

/** The plane tools show: the active tab's, else the Sidebar selection's. */
export function shownPlaneId(state: Pick<State, "sessions" | "activeTab" | "selectedPlaneId">) {
  return activeSession(state, state.selectedPlaneId)?.planeId ?? state.selectedPlaneId;
}

/** `N · title`, N being the tab's position (spec §4.2). */
export function tabLabel(session: SessionInfo, position: number): string {
  return `${position} · ${sessionTitle(session)}`;
}

export function sessionTitle(session: SessionInfo): string {
  return session.title ?? session.shellName;
}

export function isExpanded(state: Pick<State, "sessions" | "collapsed">, planeId: string): boolean {
  return sessionsOf(state, planeId).length > 0 && !state.collapsed[planeId];
}
