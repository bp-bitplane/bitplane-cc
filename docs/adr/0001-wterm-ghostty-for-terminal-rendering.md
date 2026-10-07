# ADR 0001: wterm with the Ghostty core renders sessions

Status: accepted
Date: 2026-10-07

## Context

Every session in bitplane-cc is a shell in a terminal, and the app is expected to
host Claude Code, editors and build output all day. The terminal renderer is the
one choice the whole app leans on, and it is expensive to change later, so it was
worth proving before the build rather than after.

[wterm](https://wterm.dev/) was the candidate: a DOM-rendering terminal emulator
with libghostty compiled to WASM behind it. It is also Apache-2.0, six months
old, pre-1.0 and had no published throughput benchmarks and no known Tauri user.
The alternative was xterm.js, which is mature but has no equivalent of the
Ghostty core's VT coverage.

The open questions were whether DOM rendering holds up under sustained TUI output
inside WKWebView, what many idle background sessions cost, and whether
`renderingPaused` is actually safe for a session whose tab is not on screen.

## Decision

Render sessions with `@wterm/react` on the `@wterm/ghostty` core. No xterm.js
fallback.

### What was measured

A throwaway Tauri harness ran seven scenarios end to end and wrote its own
numbers. The working notes live in the local issue tracker, which is gitignored
and will not be in your clone — so the method and the results are recorded here
instead.

Measured configuration, which is what the numbers below are evidence *for*:

| | |
|---|---|
| `@wterm/core`, `@wterm/dom`, `@wterm/ghostty`, `@wterm/react` | 0.5.4 |
| `tauri` | 2.12.1 |
| `portable-pty` | 0.9 |
| Platform | macOS 26.5.1, M-series arm64, 8 cores, WKWebView |
| Grid / scrollback budget | 120x40 / 4 MB unless stated |

Scenarios: a 13.9 MB coloured ANSI `cat` and an 18.2 MB plain `cat`; `htop -d 2`
for 10 s; Claude Code for ~16 s; keystroke-to-repaint over 150 samples against an
echoing PTY, idle and under load; ten `renderingPaused` sessions driven by output
generators for 30 s; seven scrollback budgets against 50,000 written lines; and a
paused session fed OSC 0, BEL, OSC 7, DSR, CPR and DECRQM.

### Results

Against criteria fixed before the run, every dimension passed:

- Keystroke to repaint: p95 **19 ms** idle, **50 ms** while another session
  flooded 14 MB, **19 ms** with ten background sessions running. Write to
  echoed-byte-parsed is 5.5 ms; the rest is waiting for the next frame.
- A 13.9 MB coloured `cat` never blocked the UI for more than **50 ms**, at
  6.8 MiB/s end to end with 4 ms average paints.
- htop and Claude Code both rendered with **zero** unhandled escape sequences and
  zero replacement characters; 24-bit colour, bold, box drawing, wide glyphs and
  emoji all survived to the cell grid.
- Ten `renderingPaused` sessions painted **zero** frames while still parsing
  their output and retaining scrollback. Total RSS stayed flat at ~420 MB over
  30 s; foreground typing was unaffected.
- Separately, a single `renderingPaused` session still delivered its title, bell
  and working directory to the host and still answered DSR, CPR and DECRQM
  queries, all with zero paints. Delivery happens inside `write()` rather than
  the render loop, which is what makes this work.

The parser turned out to sustain ~27 MiB/s while the harness transport delivered
~7 MiB/s, so the terminal is not the throughput ceiling — the output flow control
is, and that is ours to tune.

### What these numbers are not

One machine, one grid size, one run of each scenario: enough for a go/no-go, not
a regression baseline. Animation frames ran at ~30 Hz because the window was not
focused, so every latency figure carries up to one 33 ms frame of vsync wait and
would improve on a focused 60 Hz window. `performance.now()` is clamped to 1 ms
in WKWebView, so sub-millisecond figures come from timing a burst and dividing.
Rendering fidelity was checked at the cell and DOM level, not visually.

## Consequences

- **Pin the exact `@wterm/*` versions in the table above.** Pre-1.0 and moving
  fast; an upgrade invalidates this evidence and needs re-measuring.
- **The PTY reader must flush on a timer, not when the next read returns.**
  Coalescing inside the read loop means held bytes are only released when more
  data arrives, so a short burst followed by silence sits in the buffer
  indefinitely. This bit the harness and cost ~170 ms of keystroke latency until
  the reader and the flusher were split, with the flusher waiting on a timeout.
  The 19 ms figure above depends on that shape.
- **Scrollback is a byte budget with a floor and a large multiplier.** Anything
  under ~1 MB retains the same ~700 rows, and each retained row costs ~2.7-3.2 KB
  of WASM at 120 columns. The budget is a deliberate memory decision per session,
  not a default to accept. WASM memory never shrinks.
- **Budget ~7.5 MB of WASM per open session** at a 4 MB scrollback budget, on top
  of WebKit's own footprint.
- **`readText()` is O(n) and slow** (17.5 s for 40k rows, 4.1 s for 9.8k).
  Select-all, copy-all and search over full history cannot sit on a synchronous
  path.
- **Vite must not pre-bundle `@wterm/*`.** The Ghostty core locates its binary
  with `new URL(..., import.meta.url)`, which breaks when rewritten into
  `.vite/deps`. Add the packages to `optimizeDeps.exclude`; production builds
  emit the asset correctly.
- Reopening this decision means reopening the renderer choice generally, and
  re-running the scenarios listed above.
