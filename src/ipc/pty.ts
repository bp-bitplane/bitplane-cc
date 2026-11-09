/**
 * The raw half of the terminal transport (ADR 0003), which specta can't type:
 * output arrives as `ArrayBuffer` frames on a `Channel`, and input goes out as
 * a raw request body with the session id in a header.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

/** The first byte of every frame: what follows is a snapshot or output. */
export const FRAME_SNAPSHOT = 0;
export const FRAME_OUTPUT = 1;

export type FrameKind = typeof FRAME_SNAPSHOT | typeof FRAME_OUTPUT;

/**
 * Attach at the terminal's size. The first frame is a snapshot of the
 * session, scrollback included; a later one means the terminal starts over.
 */
export function attachSession(
  id: number,
  cols: number,
  rows: number,
  onFrame: (kind: FrameKind, bytes: Uint8Array) => void,
): Promise<void> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (payload) => {
    const frame = new Uint8Array(payload);
    const kind = frame[0] === FRAME_SNAPSHOT ? FRAME_SNAPSHOT : FRAME_OUTPUT;
    onFrame(kind, frame.subarray(1));
  };
  return invoke("session_attach", { id, cols, rows, channel });
}

const encoder = new TextEncoder();

export function ptyWrite(id: number, data: string | Uint8Array): Promise<void> {
  const bytes = typeof data === "string" ? encoder.encode(data) : data;
  return invoke("pty_write", bytes, { headers: { "x-session-id": String(id) } });
}
