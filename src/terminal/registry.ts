/**
 * wterm instances live outside React state (spec §7.1), keyed by session id.
 * Each entry owns its Ghostty core and the session's output channel.
 */
import { GhosttyCore } from "@wterm/ghostty";
import type { TerminalHandle } from "@wterm/react";
import { attachSession, commands, FRAME_SNAPSHOT, type FrameKind } from "../ipc";

/** Fixed per session (spec §9); budgets under ~1 MB don't take effect. */
export const SCROLLBACK_BYTES = 8 * 1024 * 1024;

/** Full reset, then erase the scrollback: a snapshot redraws both. */
export const RESET = "\x1bc\x1b[3J";

interface Entry {
  core: Promise<GhosttyCore>;
  handle: TerminalHandle | null;
  /** Data bytes written since the channel was attached; the ack. */
  consumed: number;
}

const entries = new Map<number, Entry>();

/** The session's core, creating it on first use. */
export function terminalCore(id: number): Promise<GhosttyCore> {
  return entry(id).core;
}

/**
 * Called once wterm is ready: attach at its size. The daemon answers with a
 * snapshot of the session, so a reload or relaunch shows it as it is.
 */
export function attachTerminal(id: number, handle: TerminalHandle, cols: number, rows: number) {
  const current = entry(id);
  current.handle = handle;
  current.consumed = 0;
  void attachSession(id, cols, rows, (kind, bytes) => write(id, current, kind, bytes)).catch(
    (error) => console.error(`attaching session ${id}:`, error),
  );
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
  current = {
    core: GhosttyCore.load({
      scrollbackLimit: SCROLLBACK_BYTES,
      backgroundColor: "#000000",
      foregroundColor: "#f5f5f5",
    }),
    handle: null,
    consumed: 0,
  };
  entries.set(id, current);
  return current;
}

/** Write a frame, then ack it: the cumulative ack is the daemon's credit. */
function write(id: number, target: Entry, kind: FrameKind, bytes: Uint8Array): void {
  if (kind === FRAME_SNAPSHOT) target.handle?.write(RESET);
  target.handle?.write(bytes);
  target.consumed += bytes.byteLength;
  void commands.ptyAck(id, target.consumed);
}
