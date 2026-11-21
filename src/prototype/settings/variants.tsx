// PROTOTYPE (bitplane-cc#70): three structurally different Settings views. Throwaway.
import { useMemo, useState } from "react";
import {
  applyOps,
  type FileValue,
  type HostSettings,
  type Op,
  resolve,
  setAccent,
  type Table,
  useSettings,
} from "./model";
import s from "./proto.module.css";
import { AccentPicker, HostNotices, Unknown } from "./shared";

const TABLES: { table: Table; title: string; hint: string }[] = [
  { table: "env", title: "Environment — every session", hint: "[env]" },
  { table: "agentClaudeEnv", title: "Environment — Claude terminals", hint: "[agent.claude.env]" },
];

function useResolved(host: HostSettings) {
  return useMemo(
    () => ({
      effective: resolve(host.loaded, !!host.captureError),
      next: resolve(host.file, !!host.captureError),
    }),
    [host],
  );
}

function show(v: string | null | undefined) {
  return v === undefined ? "—" : v === null ? "unset" : v === "" ? '""' : v;
}

/* ───────────── A: modal, left nav, form with Save / Discard ───────────── */

export function VariantA() {
  const st = useSettings();
  const [section, setSection] = useState<string>("cc");
  const [open, setOpen] = useState(true);
  if (!open)
    return (
      <div className={s.fakeMain}>
        <button type="button" className="primary" onClick={() => setOpen(true)}>
          ⌘, Settings
        </button>
      </div>
    );
  const host = st.hosts.find((h) => h.hostId === section);
  return (
    <div className={s.fakeMain}>
      <div className={s.overlay} onMouseDown={(e) => e.target === e.currentTarget && setOpen(false)}>
        <div className={s.bigDialog}>
          <nav className={s.nav}>
            <div className={s.navHead}>Settings</div>
            <button type="button" data-on={section === "cc"} onClick={() => setSection("cc")}>
              cc
            </button>
            <div className={s.navGroup}>Hosts</div>
            {st.hosts.map((h) => (
              <button
                type="button"
                key={h.hostId}
                data-on={section === h.hostId}
                onClick={() => setSection(h.hostId)}
              >
                {h.label}
                {(h.fileStatus.kind === "invalid" || h.captureError) && <span className={s.dotErr} />}
                {h.revision !== h.loadedRevision && <span className={s.dotWarn} />}
              </button>
            ))}
          </nav>
          <div className={s.pane}>
            {section === "cc" ? (
              <>
                <h2>cc</h2>
                <label className={s.fieldLabel}>Accent colour</label>
                <AccentPicker accent={st.accent} onPick={setAccent} />
              </>
            ) : (
              host && <HostFormA key={`${host.hostId}-${st.scenario}`} host={host} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function HostFormA({ host }: { host: HostSettings }) {
  const { effective, next } = useResolved(host);
  const [draft, setDraft] = useState(() => structuredClone(host.file));
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<string | null>(null);
  const ops = diff(host.file, draft);
  const locked = host.fileStatus.kind === "invalid";
  const warn = (key: string) => next.warnings.filter((w) => w.key === key);

  const save = () => {
    const err = applyOps(host.hostId, host.revision, ops);
    if (err === "SettingsChanged") setConflict(err);
    else setError(err);
  };

  return (
    <>
      <h2>
        {host.label} <span className={s.pathHint}>{host.path}</span>
      </h2>
      <HostNotices host={host} conflict={conflict} />
      <fieldset disabled={locked} className={s.fieldset}>
        {TABLES.map(({ table, title, hint }) => (
          <section key={table}>
            <div className={s.subhead}>
              {title} <span className={s.pathHint}>{hint}</span>
            </div>
            {Object.entries(draft[table]).map(([k, v]) => (
              <div key={k} className={s.formRow}>
                <input className={s.mono} value={k} readOnly style={{ width: 170 }} />
                {v === false ? (
                  <div className={s.unsetPill}>unset</div>
                ) : (
                  <input
                    className={s.mono}
                    style={{ flex: 1 }}
                    value={v}
                    onChange={(e) => setDraft(edit(draft, table, k, e.target.value))}
                  />
                )}
                <label className={s.check} title="Unset this variable (VAR = false)">
                  <input
                    type="checkbox"
                    checked={v === false}
                    onChange={(e) => setDraft(edit(draft, table, k, e.target.checked ? false : ""))}
                  />
                  unset
                </label>
                <button type="button" onClick={() => setDraft(drop(draft, table, k))}>
                  ✕
                </button>
                <div className={s.formMeta}>
                  <span>in effect: {show(effective[table][k])}</span>
                  {next[table][k] !== effective[table][k] && (
                    <span className={s.nextVal}>after restart: {show(next[table][k])}</span>
                  )}
                  {warn(`${table === "env" ? "env." : "agent.claude.env."}${k}`).map((w) => (
                    <span key={w.message} className={s.warnText}>
                      ⚠ {w.message}
                    </span>
                  ))}
                </div>
              </div>
            ))}
            <AddRow onAdd={(k) => setDraft(edit(draft, table, k, ""))} />
          </section>
        ))}
        <section>
          <div className={s.subhead}>Launching</div>
          <Scalar
            label="claude binary"
            hint="[agent.claude] command"
            value={draft.command ?? ""}
            placeholder={`look up on PATH → ${next.command.path}`}
            onChange={(v) => setDraft({ ...draft, command: v || null })}
            meta={`in effect: ${effective.command.path} (${effective.command.source})`}
          />
          <Scalar
            label="Shell"
            hint="[terminal] shell"
            value={draft.shell ?? ""}
            placeholder="$SHELL → account → /bin/sh"
            onChange={(v) => setDraft({ ...draft, shell: v || null })}
            meta={`in effect: ${effective.shell.path} (${effective.shell.source})`}
          />
          <div className={s.formRow}>
            <span className={s.scalarLabel}>Login-shell capture</span>
            <label className={s.check}>
              <input
                type="checkbox"
                checked={draft.captureEnabled}
                onChange={(e) => setDraft({ ...draft, captureEnabled: e.target.checked })}
              />
              enabled
            </label>
            <input
              type="number"
              style={{ width: 70 }}
              value={draft.captureTimeout}
              onChange={(e) => setDraft({ ...draft, captureTimeout: Number(e.target.value) })}
            />
            <span className="muted">s timeout</span>
          </div>
        </section>
        <Unknown host={host} />
      </fieldset>
      <div className={s.saveBar} data-dirty={ops.length > 0}>
        {error && <span className={s.errText}>{error}</span>}
        <span className="muted">{ops.length ? `${ops.length} unsaved change(s)` : "No unsaved changes"}</span>
        <button type="button" disabled={!ops.length} onClick={() => setDraft(structuredClone(host.file))}>
          Discard
        </button>
        <button type="button" className="primary" disabled={!ops.length} onClick={save}>
          Save
        </button>
      </div>
    </>
  );
}

/* ───────────── B: page in the main area, table, save per row ───────────── */

export function VariantB() {
  const st = useSettings();
  return (
    <div className={s.page}>
      <header className={s.pageHead}>
        <span>Settings</span>
        <span className="muted">a tab like any other · ⌘,</span>
      </header>
      <div className={s.pageScroll}>
        <section className={s.card}>
          <h2>cc</h2>
          <AccentPicker accent={st.accent} onPick={setAccent} />
        </section>
        {st.hosts.map((h) => (
          <HostTableB key={`${h.hostId}-${st.scenario}`} host={h} />
        ))}
      </div>
    </div>
  );
}

function HostTableB({ host }: { host: HostSettings }) {
  const { effective, next } = useResolved(host);
  const [conflict, setConflict] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const locked = host.fileStatus.kind === "invalid";
  const send = (ops: Op[]) => {
    const e = applyOps(host.hostId, host.revision, ops);
    if (e === "SettingsChanged") setConflict(e);
    else setErr(e);
  };
  return (
    <section className={s.card}>
      <h2>
        {host.label} <span className={s.pathHint}>{host.path}</span>
      </h2>
      <HostNotices host={host} conflict={conflict} />
      {err && <div className={s.errText}>{err}</div>}
      {TABLES.map(({ table, title, hint }) => (
        <div key={table}>
          <div className={s.subhead}>
            {title} <span className={s.pathHint}>{hint}</span>
          </div>
          <table className={s.table}>
            <thead>
              <tr>
                <th>Variable</th>
                <th>In the file</th>
                <th>In effect</th>
                <th>After restart</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {Object.entries(host.file[table]).map(([k, v]) => {
                const wkey = `${table === "env" ? "env." : "agent.claude.env."}${k}`;
                const ws = next.warnings.filter((w) => w.key === wkey);
                const changed = next[table][k] !== effective[table][k];
                return (
                  <tr key={k} data-changed={changed}>
                    <td className={s.mono}>{k}</td>
                    <td>
                      <CellEdit
                        disabled={locked}
                        value={v}
                        onCommit={(nv) => send([{ op: "set", table, key: k, value: nv }])}
                      />
                      {ws.map((w) => (
                        <div key={w.message} className={s.warnText}>
                          ⚠ {w.message}
                        </div>
                      ))}
                    </td>
                    <td className={s.monoDim}>{show(effective[table][k])}</td>
                    <td className={changed ? s.nextVal : s.monoDim}>{changed ? show(next[table][k]) : "same"}</td>
                    <td>
                      <button
                        type="button"
                        disabled={locked}
                        title="Remove from the file"
                        onClick={() => send([{ op: "remove", table, key: k }])}
                      >
                        ✕
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {!locked && <AddRow onAdd={(k) => send([{ op: "set", table, key: k, value: "" }])} />}
        </div>
      ))}
      <div className={s.subhead}>Launching</div>
      <table className={s.table}>
        <tbody>
          <tr>
            <td>claude binary</td>
            <td>
              <CellEdit
                disabled={locked}
                value={host.file.command ?? ""}
                placeholder="look up on PATH"
                onCommit={(nv) => send([{ op: "setScalar", field: "command", value: nv || null }])}
              />
            </td>
            <td className={s.monoDim}>
              {effective.command.path} ({effective.command.source})
            </td>
            <td />
          </tr>
          <tr>
            <td>Shell</td>
            <td>
              <CellEdit
                disabled={locked}
                value={host.file.shell ?? ""}
                placeholder="$SHELL → account → /bin/sh"
                onCommit={(nv) => send([{ op: "setScalar", field: "shell", value: nv || null }])}
              />
            </td>
            <td className={s.monoDim}>
              {effective.shell.path} ({effective.shell.source})
            </td>
            <td />
          </tr>
          <tr>
            <td>Login-shell capture</td>
            <td>
              <label className={s.check}>
                <input
                  type="checkbox"
                  disabled={locked}
                  checked={host.file.captureEnabled}
                  onChange={(e) => send([{ op: "setScalar", field: "captureEnabled", value: e.target.checked }])}
                />
                enabled, {host.file.captureTimeout}s timeout
              </label>
            </td>
            <td className={s.monoDim}>{host.captureError ? `failed: ${host.captureError}` : "ok"}</td>
            <td />
          </tr>
        </tbody>
      </table>
      <Unknown host={host} />
    </section>
  );
}

/** Saves on Enter or blur; a value starting with `!` would be confusing, so unset is a toggle. */
function CellEdit({
  value,
  onCommit,
  disabled,
  placeholder,
}: {
  value: FileValue;
  onCommit: (v: FileValue) => void;
  disabled?: boolean;
  placeholder?: string;
}) {
  const [text, setText] = useState(value === false ? "" : value);
  const unset = value === false;
  return (
    <div className={s.cell}>
      {unset ? (
        <span className={s.unsetPill}>unset</span>
      ) : (
        <input
          className={s.mono}
          disabled={disabled}
          value={text}
          placeholder={placeholder}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => text !== value && onCommit(text)}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
      )}
      {placeholder === undefined && (
        <button
          type="button"
          disabled={disabled}
          title={unset ? "Set a value instead" : "Unset (VAR = false)"}
          onClick={() => onCommit(unset ? "" : false)}
        >
          {unset ? "set" : "unset"}
        </button>
      )}
    </div>
  );
}

/* ───────────── C: right-hand panel, TOML-ish text per table ───────────── */

export function VariantC() {
  const st = useSettings();
  const [hostId, setHostId] = useState(st.hosts[0].hostId);
  const host = st.hosts.find((h) => h.hostId === hostId) ?? st.hosts[0];
  return (
    <div className={s.withPanel}>
      <div className={s.fakeTerminal}>
        <pre>{"~/planes/alpha ❯ claude\n\n  (a session keeps running while you edit settings beside it)"}</pre>
      </div>
      <aside className={s.panel}>
        <div className={s.panelHead}>
          Settings
          <select value={hostId} onChange={(e) => setHostId(e.target.value)}>
            {st.hosts.map((h) => (
              <option key={h.hostId} value={h.hostId}>
                {h.label}
              </option>
            ))}
          </select>
        </div>
        <div className={s.panelScroll}>
          <details open>
            <summary>cc</summary>
            <AccentPicker accent={st.accent} onPick={setAccent} />
          </details>
          <HostTextC key={`${host.hostId}-${st.scenario}-${host.revision}`} host={host} />
        </div>
      </aside>
    </div>
  );
}

function toText(t: Record<string, FileValue>) {
  return Object.entries(t)
    .map(([k, v]) => (v === false ? `${k} = false` : `${k} = "${v}"`))
    .join("\n");
}

function fromText(text: string): Record<string, FileValue> | string {
  const out: Record<string, FileValue> = {};
  for (const [i, raw] of text.split("\n").entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const m = line.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(false|"(.*)")$/);
    if (!m) return `line ${i + 1}: expected NAME = "value" or NAME = false`;
    out[m[1]] = m[2] === "false" ? false : (m[3] ?? "");
  }
  return out;
}

function HostTextC({ host }: { host: HostSettings }) {
  const { effective, next } = useResolved(host);
  const [texts, setTexts] = useState({ env: toText(host.file.env), agentClaudeEnv: toText(host.file.agentClaudeEnv) });
  const [conflict, setConflict] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const locked = host.fileStatus.kind === "invalid";
  const apply = () => {
    const draft = structuredClone(host.file);
    for (const { table } of TABLES) {
      const parsed = fromText(texts[table]);
      if (typeof parsed === "string") return setErr(`${table}: ${parsed}`);
      draft[table] = parsed;
    }
    const e = applyOps(host.hostId, host.revision, diff(host.file, draft));
    if (e === "SettingsChanged") setConflict(e);
    setErr(e === "SettingsChanged" ? null : e);
  };
  const dirty = TABLES.some(({ table }) => texts[table] !== toText(host.file[table]));
  return (
    <>
      <HostNotices host={host} conflict={conflict} />
      {TABLES.map(({ table, hint }) => (
        <details key={table} open>
          <summary>{hint}</summary>
          <textarea
            className={s.code}
            disabled={locked}
            spellCheck={false}
            rows={Math.max(3, texts[table].split("\n").length + 1)}
            value={texts[table]}
            onChange={(e) => setTexts({ ...texts, [table]: e.target.value })}
          />
          <div className={s.gutter}>
            {Object.keys(host.file[table]).map((k) => {
              const changed = next[table][k] !== effective[table][k];
              const ws = next.warnings.filter((w) => w.key.endsWith(`env.${k}`) && w.key.startsWith(table === "env" ? "env." : "agent."));
              return (
                <div key={k}>
                  <span className={s.mono}>{k}</span> →{" "}
                  <span className={changed ? s.nextVal : s.monoDim}>{show(changed ? next[table][k] : effective[table][k])}</span>
                  {changed && <span className="muted"> (now {show(effective[table][k])})</span>}
                  {ws.map((w) => (
                    <div key={w.message} className={s.warnText}>
                      ⚠ {w.message}
                    </div>
                  ))}
                </div>
              );
            })}
          </div>
        </details>
      ))}
      <details>
        <summary>Launching</summary>
        <div className={s.gutter}>
          <div>claude → {effective.command.path} ({effective.command.source})</div>
          <div>shell → {effective.shell.path} ({effective.shell.source})</div>
          <div>capture → {host.captureError ? `failed: ${host.captureError}` : `ok, ${host.file.captureTimeout}s`}</div>
          <div className="muted">edited as fields, like variant A</div>
        </div>
      </details>
      <Unknown host={host} />
      <div className={s.saveBar} data-dirty={dirty}>
        {err && <span className={s.errText}>{err}</span>}
        <button type="button" className="primary" disabled={!dirty || locked} onClick={apply}>
          Apply
        </button>
      </div>
    </>
  );
}

/* ───────────── helpers ───────────── */

function AddRow({ onAdd }: { onAdd: (k: string) => void }) {
  const [k, setK] = useState("");
  return (
    <div className={s.formRow}>
      <input
        className={s.mono}
        placeholder="NEW_VARIABLE"
        value={k}
        style={{ width: 170 }}
        onChange={(e) => setK(e.target.value.toUpperCase())}
        onKeyDown={(e) => e.key === "Enter" && k && (onAdd(k), setK(""))}
      />
      <button type="button" disabled={!k} onClick={() => (onAdd(k), setK(""))}>
        + Add
      </button>
    </div>
  );
}

function Scalar(p: {
  label: string;
  hint: string;
  value: string;
  placeholder: string;
  meta: string;
  onChange: (v: string) => void;
}) {
  return (
    <div className={s.formRow}>
      <span className={s.scalarLabel}>
        {p.label} <span className={s.pathHint}>{p.hint}</span>
      </span>
      <input
        className={s.mono}
        style={{ flex: 1 }}
        value={p.value}
        placeholder={p.placeholder}
        onChange={(e) => p.onChange(e.target.value)}
      />
      <div className={s.formMeta}>{p.meta}</div>
    </div>
  );
}

function edit(f: HostSettings["file"], t: Table, k: string, v: FileValue) {
  return { ...f, [t]: { ...f[t], [k]: v } };
}
function drop(f: HostSettings["file"], t: Table, k: string) {
  const { [k]: _, ...rest } = f[t];
  return { ...f, [t]: rest };
}

function diff(a: HostSettings["file"], b: HostSettings["file"]): Op[] {
  const ops: Op[] = [];
  for (const { table } of TABLES) {
    for (const [k, v] of Object.entries(b[table])) if (a[table][k] !== v) ops.push({ op: "set", table, key: k, value: v });
    for (const k of Object.keys(a[table])) if (!(k in b[table])) ops.push({ op: "remove", table, key: k });
  }
  for (const field of ["command", "shell", "captureEnabled", "captureTimeout"] as const)
    if (a[field] !== b[field]) ops.push({ op: "setScalar", field, value: b[field] });
  return ops;
}
