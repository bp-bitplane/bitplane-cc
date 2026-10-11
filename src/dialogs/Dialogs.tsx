import { CreatePlaneDialog } from "../create-plane/CreatePlaneDialog";
import { Log } from "../create-plane/CreateProgress";
import type { AgentHooks, BusyProcess } from "../ipc";
import { closeDialog, closeSession, installAgentHooks, quit, useStore } from "../store/store";
import styles from "./Dialogs.module.css";
import { DialogActions, Modal } from "./Modal";
import { Palette } from "./Palette";

/** Whichever dialog the store has open. */
export function Dialogs() {
  const dialog = useStore((s) => s.dialog);
  switch (dialog?.kind) {
    case "palette":
      return <Palette />;
    case "create":
      return (
        <CreatePlaneDialog
          initial={dialog.initial}
          failure={dialog.failure}
          duplicateOf={dialog.duplicateOf}
        />
      );
    case "createLog":
      return <CreateLogDialog planeId={dialog.planeId} />;
    case "confirmClose":
      return (
        <Modal label="Close session?" onClose={closeDialog}>
          <div className={styles.body}>
            <h3>Close session?</h3>
            <p>
              <code>{dialog.processName}</code> is still running.
            </p>
            <DialogActions>
              <button type="button" onClick={closeDialog}>
                Cancel
              </button>
              <button
                type="button"
                className="primary"
                // biome-ignore lint/a11y/noAutofocus: Enter confirms, like a native sheet
                autoFocus
                onClick={() => void closeSession(dialog.sessionId)}
              >
                Close
              </button>
            </DialogActions>
          </div>
        </Modal>
      );
    case "quit": {
      const { busy, creating } = dialog.info;
      const resuming = busy.filter((b) => b.resumes && !b.working);
      const lost = busy.filter((b) => !b.resumes || b.working);
      return (
        <Modal label="Quit?" onClose={closeDialog}>
          <div className={styles.body}>
            <h3>Quit?</h3>
            <BusyList title="Will be lost" busy={lost} />
            <BusyList title="Will resume" busy={resuming} />
            {creating.length > 0 && (
              <p>
                {creating.length} {creating.length === 1 ? "plane is" : "planes are"} still being
                created: {creating.join(", ")}
              </p>
            )}
            <DialogActions>
              <button type="button" onClick={closeDialog}>
                Cancel
              </button>
              <button
                type="button"
                className="primary"
                // biome-ignore lint/a11y/noAutofocus: Enter confirms, like a native sheet
                autoFocus
                onClick={() => void quit()}
              >
                Quit
              </button>
            </DialogActions>
          </div>
        </Modal>
      );
    }
    case "agentResume":
      return <AgentResumeDialog hooks={dialog.hooks} error={dialog.error} />;
    default:
      return null;
  }
}

function CreateLogDialog({ planeId }: { planeId: string }) {
  const log = useStore((s) => s.creates[planeId]?.log ?? "");
  return (
    <Modal label={`bp create ${planeId} log`} onClose={closeDialog} wide>
      <div className={styles.body}>
        <h3>Post-create log for {planeId}</h3>
        <div className={styles.log}>
          <Log text={log} />
        </div>
        <DialogActions>
          <button type="button" onClick={closeDialog}>
            Close
          </button>
        </DialogActions>
      </div>
    </Modal>
  );
}

function BusyList({ title, busy }: { title: string; busy: BusyProcess[] }) {
  if (busy.length === 0) return null;
  return (
    <section aria-label={title}>
      <p>{title}:</p>
      <ul>
        {busy.map((b, index) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: one plane can list the same process twice
          <li key={index}>
            {b.planeId}: <code>{b.processName}</code>
            {b.working && " (mid-turn)"}
          </li>
        ))}
      </ul>
    </section>
  );
}

function AgentResumeDialog({ hooks, error }: { hooks: AgentHooks | null; error: string | null }) {
  const installed = hooks?.status === "installed";
  return (
    <Modal label="Enable agent resume" onClose={closeDialog} wide>
      <div className={styles.body}>
        <h3>Enable agent resume</h3>
        {installed ? (
          <p>Agent resume is already enabled.</p>
        ) : (
          <p>
            Restore resumes the Claude Code sessions running in your tabs. These hooks get added to{" "}
            <code>{hooks?.path ?? "~/.claude/settings.json"}</code>, next to the hooks already
            there; outside this app they do nothing.
          </p>
        )}
        {hooks && !installed && <pre className={styles.code}>{hooks.entries}</pre>}
        {error && <p role="alert">{error}</p>}
        <DialogActions>
          <button type="button" onClick={closeDialog}>
            {installed ? "Close" : "Cancel"}
          </button>
          {hooks && !installed && (
            <button type="button" className="primary" onClick={() => void installAgentHooks()}>
              {hooks.status === "outdated" ? "Update hooks" : "Add hooks"}
            </button>
          )}
        </DialogActions>
      </div>
    </Modal>
  );
}
