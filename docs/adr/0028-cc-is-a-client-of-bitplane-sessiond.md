---
status: accepted
date: 2026-11-08
---

# cc is a client of bitplane-sessiond

Supersedes ADRs 0004 and 0005 and amends ADR 0003.

The host's daemon, `bitplane-sessiond`, owns the sessions: their PTYs, terminal state, persistence, restore and agent links. cc no longer holds a PTY. Its Rust backend keeps one protocol v1 connection to the daemon per host, shared by every window, and otherwise only window routing, UI state and what stays local to the desktop (the editor hand-off). cc starts the daemon it ships as a sidecar when none answers (sessiond ADR 0022).

**Every daemon session has a session tab**, ordered by `seq`, including sessions another client opened, and closing a tab closes the session for every client. A tab is joined to its session, and to its agent state, by the session id, which is also `BITPLANE_SESSION_ID`. Tab names go through `session.rename` and are kept apart from the OSC title.

**Quitting cc just quits.** Sessions, and the agents in them, keep running; relaunching cc or reloading the webview reattaches each tab with a snapshot, scrollback included (sessiond ADR 0025). There is no quit prompt. When the daemon itself restarts it restores the sessions as fresh shells with their agent sessions resumed (sessiond ADR 0027); cc sees a new `hello.daemon.instance`, reconnects and diffs `session.list` by id, so the tabs keep their ids.

ADR 0005 rejected a daemon because it needs a protocol, version skew handling and orphan handling, and still loses everything on a reboot. The protocol now exists for other reasons (a second client, remote hosts in M2), each version runs its own instance and the next adopts its sessions (sessiond ADR 0022), and restore stays for the reboot case. What a cc quit, crash or update cost before (the interrupted agent turn, the running build, the scrollback) is no longer lost; only a daemon restart costs that.

## Considered Options

- **Keep cc's own PTYs and restore** (ADR 0005 as is). Rejected: every quit or update kills running work, and the host stays unreachable from any other client.
- **A daemon connection per window, or from the webview itself.** Rejected: the webview has no socket access, and one backend connection can send the union of every window's watch scopes and route each event to the windows that asked for it.

## Consequences

- ADR 0003 keeps its webview ↔ backend leg; the backend relays terminal bytes between the webview and the daemon attachment, and the webview's acks pass through to `session.attachment.ack`.
- ADR 0004's "reload reattaches" holds, now with a snapshot instead of a blank terminal; its backend-owned state moves to the daemon.
- cc's `sessions`, `persist`, `agents`, `hooks` and `quit` host code goes. Agent tracking no longer needs cc's hooks in `~/.claude/settings.json`; the first launch of the new cc imports its `sessions.json` into the daemon and removes those hook entries.
- The other host logic (planes, creation, files, git) moves to the daemon's modules with the later tickets of M1; ADR 0006 moves with git.
- Spec: [M1 — bitplane-sessiond locally, with bitplane-cc moved onto it](https://github.com/bp-bitplane/bitplane-protocol/issues/8); implementation: cc#54.
