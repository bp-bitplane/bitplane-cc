/**
 * The frontend store (spec §7.1): a mirror of the backend's state, kept
 * current by full-state events, plus UI state that lives only here.
 */
import { create } from "zustand";
import {
  type AgentHooks,
  type CreateArgs,
  type CreateEvent,
  type CreateInfo,
  commands,
  type Envelope,
  events,
  type PlaneRow,
  type PlanesModel,
  type QuitInfo,
  type Repo,
  type SessionEvent,
  type SessionInfo,
  type Snapshot,
  type SourceControlEvent,
} from "../ipc";
import { detectPlatform, type Platform } from "../keymap";
import { toolPanelView } from "../tool-bar/toolPanelView";
import type { Tool } from "../tool-bar/tools";
import { findPlane, sessionsOf } from "./selectors";
import { fromSaved, SAVE_DELAY_MS, toSaved } from "./uiState";

export const DEFAULT_FONT_SIZE = 12;
const MIN_FONT_SIZE = 8;
const MAX_FONT_SIZE = 32;
export const DEFAULT_TOOL_PANEL_WIDTH = 320;
export const MIN_TOOL_PANEL_WIDTH = 220;
export const MAX_TOOL_PANEL_WIDTH = 640;

export type Dialog =
  | { kind: "palette" }
  | {
      kind: "create";
      initial: CreateArgs | null;
      failure: CreateFailure | null;
      duplicateOf: string | null;
    }
  | { kind: "createLog"; planeId: string }
  | { kind: "confirmClose"; sessionId: number; processName: string }
  | { kind: "quit"; info: QuitInfo }
  | { kind: "agentResume"; hooks: AgentHooks | null; error: string | null };

export interface CreateFailure {
  envelope: Envelope | null;
  /** bp's log, shown for `create_aborted`. */
  log: string | null;
  retryable: boolean;
}

export interface State {
  platform: Platform;
  planes: PlanesModel;
  sessions: Record<number, SessionInfo>;
  creates: Record<string, CreateInfo>;
  /** New plane id → the plane it duplicates, while its create runs. */
  duplicates: Record<string, string>;

  selectedPlaneId: string | null;
  activeTab: Record<string, number>;
  /** Planes the user collapsed; a plane with sessions starts expanded. */
  collapsed: Record<string, boolean>;
  dialog: Dialog | null;
  fontSize: number;
  /** Planes showing the dismissible "Post-create script failed" banner. */
  scriptFailed: Record<string, true>;
  notice: string | null;
  toolPanelOpen: boolean;
  /** The tool the panel shows when open; kept while it's collapsed. */
  activeTool: Tool;
  toolPanelWidth: number;
  sourceControl: SourceControl | null;
  /** Repo path → collapsed in Source control; never saved. */
  repoCollapsed: Record<string, boolean>;
}

/** The last Source control read, for one plane. */
export interface SourceControl {
  planeId: string;
  /** `null` until the first read for this plane succeeds. */
  repos: Repo[] | null;
  error: string | null;
  loading: boolean;
}

export const initialState = (): State => ({
  platform: detectPlatform(),
  planes: {
    status: { kind: "loading" },
    planesDir: null,
    planesDirExists: false,
    refreshedAt: null,
    envWarning: null,
    planes: [],
  },
  sessions: {},
  creates: {},
  duplicates: {},
  selectedPlaneId: null,
  activeTab: {},
  collapsed: {},
  dialog: null,
  fontSize: DEFAULT_FONT_SIZE,
  scriptFailed: {},
  notice: null,
  toolPanelOpen: false,
  activeTool: "sourceControl",
  toolPanelWidth: DEFAULT_TOOL_PANEL_WIDTH,
  sourceControl: null,
  repoCollapsed: {},
});

export const useStore = create<State>()(initialState);

const get = useStore.getState;

/** Sessions already reported closed, so a late `session_open` reply can't revive one. */
const closedSessions = new Set<number>();
const set = useStore.setState;

/** Bumped per Source control read, so a slow reply for another plane is dropped. */
let sourceControlRead = 0;

/** Load the snapshot and follow backend events. Returns an unsubscribe. */
export async function connect(onQuitRequested: () => void = requestQuit): Promise<() => void> {
  const unlisten = await Promise.all([
    events.planesChanged.listen((e) => applyPlanes(e.payload)),
    events.sessionChanged.listen((e) => applySession(e.payload)),
    events.createChanged.listen((e) => applyCreate(e.payload)),
    events.sourceControlChanged.listen((e) => applySourceControl(e.payload)),
    events.quitRequested.listen(() => onQuitRequested()),
  ]);
  loadSnapshot(await commands.snapshot());
  loadUiState(await commands.uiStateLoad().catch(() => null));
  const stopSaving = saveUiStateOnChange();
  return () => {
    stopSaving();
    for (const stop of unlisten) stop();
  };
}

export function loadUiState(json: string | null): void {
  const saved = fromSaved(json, get().sessions);
  if (saved.fontSize !== undefined) saved.fontSize = clampFontSize(saved.fontSize);
  if (saved.toolPanelWidth !== undefined) {
    saved.toolPanelWidth = clampToolPanelWidth(saved.toolPanelWidth);
  }
  set((state) => ({ ...saved, activeTab: { ...state.activeTab, ...saved.activeTab } }));
}

/** Save the UI state shortly after it changes. Returns an unsubscribe. */
export function saveUiStateOnChange(): () => void {
  let saved = JSON.stringify(toSaved(get()));
  let timer: ReturnType<typeof setTimeout> | undefined;
  const unsubscribe = useStore.subscribe(() => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      const json = JSON.stringify(toSaved(get()));
      if (json === saved) return;
      saved = json;
      void commands.uiStateSave(json).catch(() => undefined);
    }, SAVE_DELAY_MS);
  });
  return () => {
    clearTimeout(timer);
    unsubscribe();
  };
}

export function loadSnapshot(snapshot: Snapshot): void {
  const sessions = Object.fromEntries(snapshot.sessions.map((s) => [s.id, s]));
  const creates = Object.fromEntries(snapshot.creates.map((c) => [c.id, c]));
  const scriptFailed: Record<string, true> = {};
  for (const create of snapshot.creates) {
    if (create.status.kind === "scriptFailed") scriptFailed[create.id] = true;
  }
  const skipped = snapshot.restoreSkipped;
  set({
    planes: snapshot.planes,
    sessions,
    creates,
    scriptFailed,
    ...(skipped.length > 0 && {
      notice: `Couldn't restore the sessions of ${skipped.join(", ")}: ${skipped.length === 1 ? "the plane is" : "the planes are"} gone.`,
    }),
  });
}

export function applyPlanes(planes: PlanesModel): void {
  set({ planes });
}

export function applySession({ id, session }: SessionEvent): void {
  const state = get();
  const previous = state.sessions[id];
  if (!session) {
    closedSessions.add(id);
    if (!previous) return;
    const siblings = sessionsOf(state, previous.planeId);
    const index = siblings.findIndex((s) => s.id === id);
    const remaining = siblings.filter((s) => s.id !== id);
    const { [id]: _, ...sessions } = state.sessions;
    const activeTab = { ...state.activeTab };
    if (activeTab[previous.planeId] === id) {
      const neighbour = remaining[Math.min(index, remaining.length - 1)];
      if (neighbour) activeTab[previous.planeId] = neighbour.id;
      else delete activeTab[previous.planeId];
    }
    set({ sessions, activeTab });
    return;
  }
  const activeTab = state.activeTab[session.planeId]
    ? state.activeTab
    : { ...state.activeTab, [session.planeId]: id };
  set({ sessions: { ...state.sessions, [id]: session }, activeTab });
}

export function applyCreate({ id, create }: CreateEvent): void {
  const state = get();
  const previous = state.creates[id];
  if (!create) {
    const { [id]: _, ...creates } = state.creates;
    set({ creates });
    return;
  }
  set({ creates: { ...state.creates, [id]: create } });
  if (previous?.status.kind === "running" && create.status.kind !== "running") {
    void onCreateFinished(create);
  }
}

export function selectPlane(planeId: string): void {
  set({ selectedPlaneId: planeId, notice: null });
  const state = get();
  const plane = findPlane(state, planeId);
  // A plane missing from the list was most likely just created and isn't
  // listed yet; the backend still knows where its root is.
  const openable = !plane || plane.kind === "plane" || plane.kind === "draft";
  if (openable && sessionsOf(state, planeId).length === 0) void openSession(planeId);
}

export function selectSession(planeId: string, sessionId: number): void {
  set((state) => ({
    selectedPlaneId: planeId,
    activeTab: { ...state.activeTab, [planeId]: sessionId },
    notice: null,
  }));
}

export async function openSession(planeId = get().selectedPlaneId): Promise<void> {
  if (!planeId) return;
  const plane = findPlane(get(), planeId);
  if (plane && (plane.kind === "gone" || plane.kind === "creating")) return;
  try {
    const session = await commands.sessionOpen(planeId);
    // Events are authoritative; the reply only fills in when none arrived yet.
    if (!get().sessions[session.id] && !closedSessions.has(session.id)) {
      applySession({ id: session.id, session });
    }
    if (closedSessions.has(session.id)) return;
    set((state) => ({ activeTab: { ...state.activeTab, [planeId]: session.id } }));
  } catch (error) {
    set({ notice: String(error) });
  }
}

/** Close a session, asking first when something other than the shell runs. */
export async function requestCloseSession(sessionId = activeSessionId()): Promise<void> {
  if (sessionId === undefined) return;
  const session = get().sessions[sessionId];
  if (!session) return;
  if (session.status.kind === "running") {
    const busy = await commands.sessionBusy(sessionId).catch(() => null);
    if (busy?.busy) {
      set({
        dialog: {
          kind: "confirmClose",
          sessionId,
          processName: busy.processName ?? "A process",
        },
      });
      return;
    }
  }
  await closeSession(sessionId);
}

export async function closeSession(sessionId: number): Promise<void> {
  if (get().dialog?.kind === "confirmClose") set({ dialog: null });
  await commands.sessionClose(sessionId);
}

export async function restartSession(sessionId: number): Promise<void> {
  try {
    await commands.sessionRestart(sessionId);
  } catch (error) {
    set({ notice: String(error) });
  }
}

export function cycleTab(delta: number): void {
  const state = get();
  const planeId = state.selectedPlaneId;
  if (!planeId) return;
  const tabs = sessionsOf(state, planeId);
  if (tabs.length === 0) return;
  const index = tabs.findIndex((s) => s.id === activeSessionId());
  const next = tabs[(index + delta + tabs.length) % tabs.length];
  if (next) selectSession(planeId, next.id);
}

export function goToTab(position: number): void {
  const state = get();
  const planeId = state.selectedPlaneId;
  if (!planeId) return;
  const target = sessionsOf(state, planeId)[position - 1];
  if (target) selectSession(planeId, target.id);
}

export function cyclePlane(delta: number): void {
  const rows = get().planes.planes;
  if (rows.length === 0) return;
  const index = rows.findIndex((p) => p.id === get().selectedPlaneId);
  const start = index === -1 ? (delta > 0 ? -1 : 0) : index;
  const next = rows[(start + delta + rows.length) % rows.length];
  if (next) selectPlane(next.id);
}

export function goToPlane(position: number): void {
  const target = get().planes.planes[position - 1];
  if (target) selectPlane(target.id);
}

export function toggleCollapsed(planeId: string, collapsed: boolean): void {
  set((state) => ({ collapsed: { ...state.collapsed, [planeId]: collapsed } }));
}

export function setAllCollapsed(collapsed: boolean): void {
  const rows = get().planes.planes;
  set({ collapsed: Object.fromEntries(rows.map((p) => [p.id, collapsed])) });
}

export function openDialog(dialog: Dialog): void {
  set({ dialog });
}

export function togglePalette(): void {
  set((state) => ({ dialog: state.dialog?.kind === "palette" ? null : { kind: "palette" } }));
}

export function openCreateDialog(): void {
  set({ dialog: { kind: "create", initial: null, failure: null, duplicateOf: null } });
}

/** Branch and id start empty: a branch can't be checked out in two worktrees at once. */
export function openDuplicateDialog(plane: PlaneRow): void {
  set({
    dialog: {
      kind: "create",
      initial: {
        id: "",
        branch: "",
        members: plane.members.map((spec) => ({ spec, branch: null })),
        intent: "auto",
        noFetch: false,
        noScripts: false,
      },
      failure: null,
      duplicateOf: plane.id,
    },
  });
}

export function closeDialog(): void {
  set({ dialog: null });
}

export function dismissNotice(): void {
  set({ notice: null });
}

/** Capture the environment again and re-read the plane list (also every Retry). */
export async function refresh(): Promise<void> {
  const planeId = sourceControlPlaneId();
  await Promise.all([commands.refresh(), planeId && loadSourceControl(planeId)]);
}

/** `mkdir` a draft plane, then select it, which opens a session. */
export async function createDraft(id: string): Promise<void> {
  await commands.planeCreateDraft(id);
  closeDialog();
  selectPlane(id);
}

export async function createPlane(
  args: CreateArgs,
  duplicateOf: string | null = null,
): Promise<void> {
  await commands.planeCreate(args);
  if (duplicateOf !== null) {
    set((state) => ({ duplicates: { ...state.duplicates, [args.id]: duplicateOf } }));
  }
  closeDialog();
  // Not `selectPlane`: the "creating" row may not have arrived yet, and a
  // shell must never start in a half-built plane.
  set({ selectedPlaneId: args.id, notice: null });
}

export async function cancelCreate(planeId: string): Promise<void> {
  await commands.planeCreateCancel(planeId);
}

export async function dismissCreate(planeId: string): Promise<void> {
  set((state) => {
    const { [planeId]: _, ...scriptFailed } = state.scriptFailed;
    return { scriptFailed };
  });
  await commands.planeCreateDismiss(planeId);
}

export function zoom(step: 1 | -1 | 0): void {
  set((state) => ({
    fontSize: step === 0 ? DEFAULT_FONT_SIZE : clampFontSize(state.fontSize + step),
  }));
}

/** Clicking the active tool collapses the panel; any other tool opens in it. */
export function selectTool(tool: Tool): void {
  set((state) =>
    state.toolPanelOpen && state.activeTool === tool
      ? { toolPanelOpen: false }
      : { toolPanelOpen: true, activeTool: tool },
  );
}

export function toggleToolPanel(): void {
  set((state) => ({ toolPanelOpen: !state.toolPanelOpen }));
}

export function setToolPanelWidth(width: number): void {
  set({ toolPanelWidth: clampToolPanelWidth(width) });
}

export async function loadSourceControl(planeId: string): Promise<void> {
  const read = ++sourceControlRead;
  set((state) => ({
    sourceControl: {
      planeId,
      repos: state.sourceControl?.planeId === planeId ? state.sourceControl.repos : null,
      error: null,
      loading: true,
    },
  }));
  try {
    const repos = await commands.planeSourceControl(planeId);
    if (read === sourceControlRead) {
      set({ sourceControl: { planeId, repos, error: null, loading: false } });
    }
  } catch (error) {
    if (read === sourceControlRead) {
      set({ sourceControl: { planeId, repos: null, error: String(error), loading: false } });
    }
  }
}

/** A pushed read wins over any read still in flight. */
export function applySourceControl({ planeId, repos }: SourceControlEvent): void {
  if (get().sourceControl?.planeId !== planeId) return;
  sourceControlRead++;
  set({ sourceControl: { planeId, repos, error: null, loading: false } });
}

/** Keep `planeId`'s Source control current while it's on screen; returns a stop. */
export function watchSourceControl(planeId: string): () => void {
  void commands.planeSourceControlWatch(planeId).catch(() => undefined);
  return () => void commands.planeSourceControlWatch(null).catch(() => undefined);
}

export function toggleRepoCollapsed(path: string): void {
  set((state) => ({
    repoCollapsed: { ...state.repoCollapsed, [path]: !state.repoCollapsed[path] },
  }));
}

export function setAllReposCollapsed(collapsed: boolean): void {
  const repos = get().sourceControl?.repos ?? [];
  set((state) => ({
    repoCollapsed: {
      ...state.repoCollapsed,
      ...Object.fromEntries(repos.map((repo) => [repo.path, collapsed])),
    },
  }));
}

export async function openInVscode(planeId: string): Promise<void> {
  try {
    await commands.planeOpenInVscode(planeId);
  } catch (error) {
    set({ notice: String(error) });
  }
}

export async function openAgentResume(): Promise<void> {
  set({ dialog: { kind: "agentResume", hooks: null, error: null } });
  try {
    const hooks = await commands.agentHooksDescribe();
    set({ dialog: { kind: "agentResume", hooks, error: null } });
  } catch (error) {
    set({ dialog: { kind: "agentResume", hooks: null, error: String(error) } });
  }
}

export async function installAgentHooks(): Promise<void> {
  try {
    await commands.agentHooksInstall();
    set({ dialog: null, notice: "Agent resume is enabled for new Claude Code sessions." });
  } catch (error) {
    const dialog = get().dialog;
    const hooks = dialog?.kind === "agentResume" ? dialog.hooks : null;
    set({ dialog: { kind: "agentResume", hooks, error: String(error) } });
  }
}

/** ⌘Q or closing the window: confirm only when something would be lost. */
export async function requestQuit(): Promise<void> {
  const info = await commands.quitCheck();
  const lost = info.busy.some((b) => !b.resumes || b.working);
  if (lost || info.creating.length > 0) {
    set({ dialog: { kind: "quit", info } });
  } else {
    await commands.quit();
  }
}

export async function quit(): Promise<void> {
  await commands.quit();
}

function clampFontSize(size: number): number {
  return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, size));
}

function clampToolPanelWidth(width: number): number {
  return Math.round(Math.min(MAX_TOOL_PANEL_WIDTH, Math.max(MIN_TOOL_PANEL_WIDTH, width)));
}

function activeSessionId(): number | undefined {
  const state = get();
  const planeId = state.selectedPlaneId;
  if (!planeId) return undefined;
  const active = state.activeTab[planeId];
  const tabs = sessionsOf(state, planeId);
  return tabs.find((s) => s.id === active)?.id ?? tabs[0]?.id;
}

async function onCreateFinished(create: CreateInfo): Promise<void> {
  const { [create.id]: duplicateOf = null, ...duplicates } = get().duplicates;
  set({ duplicates });
  switch (create.status.kind) {
    case "succeeded":
      await commands.planeCreateDismiss(create.id);
      selectPlane(create.id);
      break;
    case "scriptFailed":
      set((state) => ({ scriptFailed: { ...state.scriptFailed, [create.id]: true } }));
      selectPlane(create.id);
      break;
    case "nothingCreated": {
      const { envelope, retryable } = create.status;
      await commands.planeCreateDismiss(create.id);
      if (get().selectedPlaneId === create.id) set({ selectedPlaneId: null });
      set({
        dialog: {
          kind: "create",
          initial: create.args,
          failure: {
            envelope,
            log: envelope?.error === "create_aborted" ? create.log : null,
            retryable,
          },
          duplicateOf,
        },
      });
      break;
    }
    default:
      break;
  }
}

/** The plane Source control is showing, or `null` when it isn't on screen. */
function sourceControlPlaneId(): string | null {
  const state = get();
  if (!state.toolPanelOpen || state.activeTool !== "sourceControl") return null;
  const view = toolPanelView(state);
  return view.screen === null ? view.planeId : null;
}
