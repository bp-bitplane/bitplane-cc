import type { GhosttyCore } from "@wterm/ghostty";
import { Terminal, type TerminalHandle } from "@wterm/react";
import { useEffect, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { commands, ptyWrite, type SessionInfo } from "../ipc";
import { findPlane } from "../store/selectors";
import { closeSession, restartSession, useStore } from "../store/store";
import { disposeTerminal, setHandle, terminalCore } from "./registry";
import styles from "./TerminalStack.module.css";

/**
 * Every session's terminal, stacked. Only `visibleId` paints; the rest keep
 * parsing with rendering paused, so titles, bells and replies still flow.
 */
export function TerminalStack({ visibleId }: { visibleId: number | null }) {
  const sessions = useStore(useShallow((s) => Object.values(s.sessions)));
  const fontSize = useStore((s) => s.fontSize);
  useDisposeClosed(sessions);

  return (
    <div
      className={styles.stack}
      style={{ "--term-font-size": `${fontSize}px` } as React.CSSProperties}
    >
      {sessions.map((session) => (
        <TerminalPane key={session.id} session={session} visible={session.id === visibleId} />
      ))}
    </div>
  );
}

function TerminalPane({ session, visible }: { session: SessionInfo; visible: boolean }) {
  const core = useCore(session.id);
  const ref = useRef<TerminalHandle>(null);
  const id = session.id;

  useEffect(() => {
    if (visible && core) ref.current?.focus();
  }, [visible, core]);

  return (
    <section
      className={`${styles.pane} ${visible ? styles.visible : ""}`}
      aria-hidden={!visible}
      data-session={id}
    >
      {core && (
        <Terminal
          ref={ref}
          core={core}
          autoResize
          renderingPaused={!visible}
          className={styles.terminal}
          onData={(data) => void ptyWrite(id, data)}
          onResize={(cols, rows) => void commands.ptyResize(id, cols, rows)}
          onTitle={(title) => void commands.sessionSetTitle(id, title)}
          onReady={(term) => {
            setHandle(id, ref.current);
            void commands.ptyResize(id, term.cols, term.rows);
            if (visible) ref.current?.focus();
          }}
          onError={(error) => console.error(`terminal ${id}:`, error)}
        />
      )}
      {session.status.kind !== "running" && <ExitedBar session={session} />}
    </section>
  );
}

function ExitedBar({ session }: { session: SessionInfo }) {
  const gone = useStore((s) => findPlane(s, session.planeId)?.kind === "gone");
  const failed = session.status.kind === "spawnFailed";
  return (
    <div className={styles.exited} role="status">
      <span className={styles.exitText}>
        {session.status.kind === "exited" && `[process exited with code ${session.status.code}]`}
        {session.status.kind === "spawnFailed" && session.status.error}
      </span>
      <button
        type="button"
        className="primary"
        disabled={gone}
        title={gone ? "The plane is gone" : undefined}
        onClick={() => void restartSession(session.id)}
      >
        {failed ? "Retry" : "Restart"}
      </button>
      <button type="button" onClick={() => void closeSession(session.id)}>
        Close
      </button>
    </div>
  );
}

function useCore(id: number): GhosttyCore | null {
  const [core, setCore] = useState<GhosttyCore | null>(null);
  useEffect(() => {
    let alive = true;
    void terminalCore(id).then((loaded) => {
      if (alive) setCore(loaded);
    });
    return () => {
      alive = false;
    };
  }, [id]);
  return core;
}

/** Dispose cores of sessions that left the mirror, after their pane unmounts. */
function useDisposeClosed(sessions: SessionInfo[]): void {
  const previous = useRef<Set<number>>(new Set());
  useEffect(() => {
    const current = new Set(sessions.map((s) => s.id));
    for (const id of previous.current) {
      if (!current.has(id)) disposeTerminal(id);
    }
    previous.current = current;
  }, [sessions]);
}
