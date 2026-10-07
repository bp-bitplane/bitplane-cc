import { CreatePlaneDialog } from "../create-plane/CreatePlaneDialog";
import { Log } from "../create-plane/CreateProgress";
import { closeDialog, closeSession, quit, useStore } from "../store/store";
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
      return <CreatePlaneDialog initial={dialog.initial} failure={dialog.failure} />;
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
      return (
        <Modal label="Quit?" onClose={closeDialog}>
          <div className={styles.body}>
            <h3>Quit?</h3>
            {busy.length > 0 && (
              <>
                <p>
                  {busy.length} {busy.length === 1 ? "session has" : "sessions have"} running
                  processes:
                </p>
                <ul>
                  {busy.map((b, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: one plane can list the same process twice
                    <li key={index}>
                      {b.planeId}: <code>{b.processName}</code>
                    </li>
                  ))}
                </ul>
              </>
            )}
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
