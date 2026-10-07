/**
 * wterm instances live outside React state (spec §7.1), keyed by session id.
 * Each entry owns its Ghostty core and the session's output channel.
 */
import { GhosttyCore } from "@wterm/ghostty";
import type { TerminalHandle } from "@wterm/react";
import { attachSession, commands } from "../ipc";

/** Fixed per session (spec §9); budgets under ~1 MB don't take effect. */
export const SCROLLBACK_BYTES = 8 * 1024 * 1024;

interface Entry {
  core: Promise<GhosttyCore>;
  handle: TerminalHandle | null;
  /** Frames that arrived before the terminal was ready; unacked until written. */
  backlog: Uint8Array[];
  /** Bytes written into the terminal since the channel was attached. */
  consumed: number;
}

const entries = new Map<number, Entry>();

/** The session's core, creating it and attaching its channel on first use. */
export function terminalCore(id: number): Promise<GhosttyCore> {
  return entry(id).core;
}

/** Called once wterm is ready: write whatever arrived meanwhile. */
export function setHandle(id: number, handle: TerminalHandle | null): void {
  const current = entries.get(id);
  if (!current) return;
  current.handle = handle;
  if (!handle) return;
  const backlog = current.backlog.splice(0);
  for (const bytes of backlog) write(id, current, bytes);
}

/**
 * Free the core once React has unmounted the terminal: `WTerm.destroy()`
 * doesn't dispose a core that was passed in, and rendering against a
 * disposed core reads freed WASM memory.
 */
export function disposeTerminal(id: number): void {
  const current = entries.get(id);
  if (!current) return;
  entries.delete(id);
  void current.core.then((core) => setTimeout(() => core.dispose(), 50));
}

function entry(id: number): Entry {
  let current = entries.get(id);
  if (current) return current;
  const created: Entry = {
    core: GhosttyCore.load({
      scrollbackLimit: SCROLLBACK_BYTES,
      backgroundColor: "#000000",
      foregroundColor: "#f5f5f5",
    }),
    handle: null,
    backlog: [],
    consumed: 0,
  };
  current = created;
  entries.set(id, created);
  void attachSession(id, (bytes) => {
    if (created.handle) write(id, created, bytes);
    else created.backlog.push(bytes);
  }).catch((error) => console.error(`attaching session ${id}:`, error));
  return created;
}

/** Write a frame, then ack it: the cumulative ack is the backend's credit. */
function write(id: number, target: Entry, bytes: Uint8Array): void {
  target.handle?.write(bytes);
  target.consumed += bytes.byteLength;
  void commands.ptyAck(id, target.consumed);
}
