// PROTOTYPE (bitplane-cc#70): bits every variant uses. Throwaway.
import { useState } from "react";
import { DialogActions, Modal } from "../../dialogs/Modal";
import { type HostSettings, pending, reloadFixed, restart } from "./model";
import s from "./proto.module.css";

export function RestartConfirm({ host, onClose }: { host: HostSettings; onClose: () => void }) {
  const busy = host.sessions.filter((x) => x.busy);
  return (
    <Modal label="Restart daemon" onClose={onClose}>
      <div className={s.modalBody}>
        <h3>Restart the session daemon on {host.label}?</h3>
        <p className="muted">
          Every session's processes end. Sessions come back as fresh shells; Claude terminals resume
          their conversation.
        </p>
        <div className={s.sessionList}>
          {host.sessions.map((x) => (
            <div key={x.name} className={x.busy ? s.busyRow : undefined}>
              <span>{x.name}</span>
              <span className="muted">{x.plane}</span>
              {x.busy && <span className={s.busyTag}>{x.busy}</span>}
            </div>
          ))}
        </div>
        {busy.length > 0 && (
          <p className={s.warnText}>{busy.length} busy — their work in progress is interrupted.</p>
        )}
        <DialogActions>
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            className={s.danger}
            onClick={() => {
              restart(host.hostId);
              onClose();
            }}
          >
            Restart {host.sessions.length} sessions
          </button>
        </DialogActions>
      </div>
    </Modal>
  );
}

/** The host-level notices: parse error, capture failure, pending restart. */
export function HostNotices({ host, conflict }: { host: HostSettings; conflict: string | null }) {
  const [confirm, setConfirm] = useState(false);
  return (
    <>
      {host.fileStatus.kind === "invalid" && (
        <div className={s.errorBox}>
          <b>settings.toml doesn't parse</b> — line {host.fileStatus.line}:{host.fileStatus.col}:{" "}
          {host.fileStatus.message}. The daemon is running with no settings. Fix the file by hand:
          <code>{host.path}</code>
          <button type="button" onClick={() => reloadFixed(host.hostId)}>
            Reload (pretend fixed)
          </button>
        </div>
      )}
      {host.captureError && (
        <div className={s.warnBox}>
          <b>Using the daemon's own environment</b>, not your login shell's: {host.captureError}.
          See <i>Login-shell capture</i> below.
        </div>
      )}
      {conflict && (
        <div className={s.warnBox}>
          <b>The file changed on disk</b> since you opened it. Reloaded; your unsaved edits are kept
          where they don't clash — review and save again.
        </div>
      )}
      {pending(host) && (
        <div className={s.pendingBox}>
          <span>Saved, not in effect yet — restart the daemon to apply.</span>
          <button
            type="button"
            className="primary"
            disabled={host.remote}
            title={host.remote ? "Restarting a remote daemon comes in M2" : undefined}
            onClick={() => setConfirm(true)}
          >
            Restart daemon…
          </button>
        </div>
      )}
      {confirm && <RestartConfirm host={host} onClose={() => setConfirm(false)} />}
    </>
  );
}

export function Unknown({ host }: { host: HostSettings }) {
  const keys = Object.entries(host.file.unknown);
  if (!keys.length) return null;
  return (
    <div className={s.unknown}>
      <div className={s.subhead}>Set in the file, not editable here</div>
      {keys.map(([k, v]) => (
        <div key={k} className={s.mono}>
          {k} = {v} <span className="muted">— not known to this daemon; ignored</span>
        </div>
      ))}
    </div>
  );
}

export const ACCENTS = ["#4cb8b4", "#7c8cff", "#ff8a4c", "#e05cc0", "#8fd14f"];

export function AccentPicker({ accent, onPick, bare }: { accent: string; onPick: (a: string) => void; bare?: boolean }) {
  return (
    <div className={s.accentRow}>
      {ACCENTS.map((a) => (
        <button
          type="button"
          key={a}
          aria-label={a}
          className={s.swatch}
          style={{ background: a, outline: a === accent ? "2px solid var(--text)" : undefined }}
          onClick={() => onPick(a)}
        />
      ))}
      <input
        className={s.mono}
        style={{ width: 90 }}
        defaultValue={accent}
        key={accent}
        onBlur={(e) => /^#[0-9a-f]{6}$/i.test(e.target.value) && onPick(e.target.value)}
      />
      {!bare && <span className="muted">applied at once · ~/.config/bitplane/cc/settings.toml</span>}
    </div>
  );
}
