# Terminal I/O over Tauri IPC with app-level flow control

PTY output reaches the webview as raw bytes over a Tauri `Channel`, and input goes back through a raw-body command. The app adds its own flow control: cumulative byte acks with a window of about 128 KiB. A local WebSocket server (wterm's built-in transport) was rejected. It would expose a localhost port that needs a token and Origin check, and it would need a CSP exception. It would also still need the same flow-control protocol, because neither transport limits output on its own. Throughput (~7 MiB/s) is bounded by this window, not by wterm, so tune the window rather than switch transports.

## Amendment: the PTY is the daemon's (ADR 0028)

The PTY now lives in `bitplane-sessiond`, and the backend holds one daemon attachment per session tab. This IPC leg is unchanged: output still reaches the webview over a `Channel` and input still goes back through a raw-body command. The backend relays bytes between it and the attachment and no longer runs a window of its own; the webview's cumulative acks pass through as `session.attachment.ack`, so the daemon's per-attachment window (128 KiB, 32 KiB low water, sessiond ADR 0026) is the one that limits output.
