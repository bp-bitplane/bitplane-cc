import { CreatePlaneDialog } from "../create-plane/CreatePlaneDialog";
import { Log } from "../create-plane/CreateProgress";
import type { DaemonInfo } from "../ipc";
import { closeDialog, closeSession, useStore } from "../store/store";
import styles from "./Dialogs.module.css";
import { Launcher } from "./Launcher";
import { DialogActions, Modal } from "./Modal";
import { Palette } from "./Palette";

/** Whichever dialog the store has open. */
export function Dialogs() {
  const dialog = useStore((s) => s.dialog);
  switch (dialog?.kind) {
    case "palette":
      return <Palette />;
    case "launcher":
      return <Launcher planeId={dialog.planeId} />;
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
    case "daemonInfo":
      return <DaemonInfoDialog info={dialog.info} />;
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

function DaemonInfoDialog({ info }: { info: DaemonInfo | null }) {
  const status = useStore((s) => s.daemon);
  const rows: [string, string | number | null | undefined][] = [
    ["Process id", info?.pid],
    ["Executable", info?.executable],
    ["Bundled", info?.bundled],
    ["Socket", info?.socket],
    ["Log", info?.log],
  ];
  return (
    <Modal label="Session daemon" onClose={closeDialog} wide>
      <div className={styles.body}>
        <h3>Session daemon</h3>
        <dl className={styles.facts}>
          {rows.map(([label, value]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>
                <code>{value ?? "unknown"}</code>
              </dd>
            </div>
          ))}
        </dl>
        <p>Status</p>
        <pre className={styles.log}>{JSON.stringify(status, null, 2)}</pre>
        <p>Last hello reply</p>
        <pre className={styles.log}>{info?.hello ?? "No hello reply yet."}</pre>
        <DialogActions>
          <button type="button" onClick={closeDialog}>
            Close
          </button>
        </DialogActions>
      </div>
    </Modal>
  );
}
