import { mockIPC } from "@tauri-apps/api/mocks";
import type { CreateInfo, PlaneKind, PlaneRow, PlanesModel, SessionInfo } from "../ipc";
import { initialState, type State, useStore } from "../store/store";

export function plane(id: string, kind: PlaneKind = "plane", findings: string[] = []): PlaneRow {
  return { id, directory: `/planes/${id}`, kind, findings };
}

export function planesModel(rows: PlaneRow[], overrides: Partial<PlanesModel> = {}): PlanesModel {
  return {
    status: { kind: "ready" },
    planesDir: "/planes",
    planesDirExists: true,
    refreshedAt: 0,
    envWarning: null,
    planes: rows,
    ...overrides,
  };
}

export function session(
  id: number,
  planeId: string,
  overrides: Partial<SessionInfo> = {},
): SessionInfo {
  return {
    id,
    planeId,
    seq: id,
    status: { kind: "running" },
    title: null,
    shellName: "zsh",
    ...overrides,
  };
}

export function createInfo(id: string, overrides: Partial<CreateInfo> = {}): CreateInfo {
  return {
    id,
    args: {
      id,
      branch: "feat/x",
      members: [{ spec: "@api", branch: null }],
      intent: "auto",
      noFetch: false,
      noScripts: false,
    },
    directory: `/planes/${id}`,
    phase: "fetching",
    status: { kind: "running" },
    log: "",
    ...overrides,
  };
}

/** Reset the store, then apply `overrides`. */
export function seed(overrides: Partial<State> = {}): void {
  useStore.setState({ ...initialState(), platform: "mac", ...overrides }, true);
}

export function sessionsById(...sessions: SessionInfo[]): Record<number, SessionInfo> {
  return Object.fromEntries(sessions.map((s) => [s.id, s]));
}

export interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

/**
 * Mock the backend. `handlers` answer commands by name; anything else
 * resolves to null. Every call is recorded.
 */
export function mockBackend(
  handlers: Record<string, (args: Record<string, unknown>) => unknown> = {},
) {
  const calls: Call[] = [];
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      const handler = handlers[cmd];
      return handler ? handler(args) : null;
    },
    { shouldMockEvents: true },
  );
  return {
    calls,
    called: (cmd: string) => calls.filter((c) => c.cmd === cmd),
  };
}

export const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
