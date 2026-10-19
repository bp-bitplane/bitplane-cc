import type { PlaneRow, SessionInfo } from "../ipc";
import { type FileTab, fileTabKey } from "../tabs/fileTabs";
import type { State, TabId } from "./store";

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

const NO_FILE_TABS: FileTab[] = [];

export function fileTabsOf(state: Pick<State, "fileTabs">, planeId: string): FileTab[] {
  return state.fileTabs[planeId] ?? NO_FILE_TABS;
}

type Tabs = Pick<State, "sessions" | "fileTabs" | "activeTab">;

/** A plane's Tabs: Session tabs in creation order, then File tabs. */
export function tabsOf(state: Omit<Tabs, "activeTab">, planeId: string): TabId[] {
  return [
    ...sessionsOf(state, planeId).map((s) => s.id),
    ...fileTabsOf(state, planeId).map(fileTabKey),
  ];
}

/** The plane's active Tab, else its first one. */
export function activeTabId(state: Tabs, planeId: string | null): TabId | undefined {
  if (!planeId) return undefined;
  const tabs = tabsOf(state, planeId);
  const active = state.activeTab[planeId];
  return active !== undefined && tabs.includes(active) ? active : tabs[0];
}

/** `undefined` also while a File tab is active. */
export function activeSession(state: Tabs, planeId: string | null): SessionInfo | undefined {
  const id = activeTabId(state, planeId);
  return typeof id === "number" ? state.sessions[id] : undefined;
}

export function activeFileTab(state: Tabs, planeId: string | null): FileTab | undefined {
  const id = activeTabId(state, planeId);
  if (!planeId || typeof id !== "string") return undefined;
  return fileTabsOf(state, planeId).find((tab) => fileTabKey(tab) === id);
}

/** The plane tools show: the active tab's, else the Sidebar selection's. */
export function shownPlaneId(state: Tabs & Pick<State, "selectedPlaneId">) {
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
