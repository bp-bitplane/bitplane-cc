/**
 * The UI state that survives a restart (ADR 0005). Tabs are saved by session
 * key: session ids start over with every launch. File tabs aren't restored
 * (ADR 0008), so an active one falls back to the plane's last session.
 */
import type { SessionInfo } from "../ipc";
import type { FileTabKey } from "../tabs/fileTabs";
import { TOOLS, type Tool } from "../tool-bar/tools";
import { sessionsOf } from "./selectors";
import type { DiffStyle, State } from "./store";

export const SAVE_DELAY_MS = 300;

export interface SavedUiState {
  selectedPlaneId: string | null;
  /** Plane id → the active session's key, or the active File tab. */
  activeTab: Record<string, string | { file: FileTabKey }>;
  collapsed: Record<string, boolean>;
  fontSize: number;
  toolPanel: { open: boolean; tool: Tool; width: number };
  sidebar: { width: number };
  fileTab: { diffStyle: DiffStyle };
}

export function toSaved(state: State): SavedUiState {
  const activeTab: SavedUiState["activeTab"] = {};
  for (const [planeId, id] of Object.entries(state.activeTab)) {
    if (typeof id === "string") {
      activeTab[planeId] = { file: id };
      continue;
    }
    const key = state.sessions[id]?.key;
    if (key) activeTab[planeId] = key;
  }
  return {
    selectedPlaneId: state.selectedPlaneId,
    activeTab,
    collapsed: state.collapsed,
    fontSize: state.fontSize,
    toolPanel: {
      open: state.toolPanelOpen,
      tool: state.activeTool,
      width: state.toolPanelWidth,
    },
    sidebar: { width: state.sidebarWidth },
    fileTab: { diffStyle: state.diffStyle },
  };
}

/** What of `json` still applies; anything malformed is dropped. */
export function fromSaved(
  json: string | null,
  sessions: Record<number, SessionInfo>,
): Partial<
  Pick<
    State,
    | "selectedPlaneId"
    | "activeTab"
    | "collapsed"
    | "fontSize"
    | "toolPanelOpen"
    | "activeTool"
    | "toolPanelWidth"
    | "sidebarWidth"
    | "diffStyle"
  >
> {
  const saved = parse(json);
  if (!saved) return {};
  const idByKey = new Map(Object.values(sessions).map((s) => [s.key, s.id]));
  const result: ReturnType<typeof fromSaved> = {};
  if (typeof saved.selectedPlaneId === "string") result.selectedPlaneId = saved.selectedPlaneId;
  if (typeof saved.fontSize === "number" && Number.isFinite(saved.fontSize)) {
    result.fontSize = saved.fontSize;
  }
  const activeTab: Record<string, number> = {};
  for (const [planeId, value] of entries(saved.activeTab)) {
    const id =
      typeof value === "string"
        ? idByKey.get(value)
        : value && typeof value === "object"
          ? sessionsOf({ sessions }, planeId).at(-1)?.id
          : undefined;
    if (id !== undefined && sessions[id]?.planeId === planeId) activeTab[planeId] = id;
  }
  result.activeTab = activeTab;
  result.collapsed = Object.fromEntries(
    entries(saved.collapsed).filter(
      (entry): entry is [string, boolean] => typeof entry[1] === "boolean",
    ),
  );
  const toolPanel = object(saved.toolPanel);
  if (typeof toolPanel.open === "boolean") result.toolPanelOpen = toolPanel.open;
  if (TOOLS.some((tool) => tool === toolPanel.tool)) result.activeTool = toolPanel.tool as Tool;
  if (typeof toolPanel.width === "number" && Number.isFinite(toolPanel.width)) {
    result.toolPanelWidth = toolPanel.width;
  }
  const fileTab = object(saved.fileTab);
  if (fileTab.diffStyle === "unified" || fileTab.diffStyle === "split") {
    result.diffStyle = fileTab.diffStyle;
  }
  const sidebar = object(saved.sidebar);
  if (typeof sidebar.width === "number" && Number.isFinite(sidebar.width)) {
    result.sidebarWidth = sidebar.width;
  }
  return result;
}

function parse(json: string | null): Record<string, unknown> | null {
  if (!json) return null;
  try {
    const value: unknown = JSON.parse(json);
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function entries(value: unknown): [string, unknown][] {
  return Object.entries(object(value));
}

function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
