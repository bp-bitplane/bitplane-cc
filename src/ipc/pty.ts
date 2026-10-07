/**
 * The raw half of the terminal transport (ADR 0003), which specta can't type:
 * output arrives as `ArrayBuffer` frames on a `Channel`, and input goes out as
 * a raw request body with the session id in a header.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

export function attachSession(id: number, onFrame: (bytes: Uint8Array) => void): Promise<void> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (payload) => onFrame(new Uint8Array(payload));
  return invoke("session_attach", { id, channel });
}

const encoder = new TextEncoder();

export function ptyWrite(id: number, data: string | Uint8Array): Promise<void> {
  const bytes = typeof data === "string" ? encoder.encode(data) : data;
  return invoke("pty_write", bytes, { headers: { "x-session-id": String(id) } });
}
