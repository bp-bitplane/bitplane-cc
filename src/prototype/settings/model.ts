// PROTOTYPE (bitplane-cc#70 cc Settings view): throwaway, in-memory fake of settings.get/settings.set.
import { useSyncExternalStore } from "react";

export type Scenario = "clean" | "pending" | "invalid" | "captureFailed" | "conflict";
export const SCENARIOS: Scenario[] = ["clean", "pending", "invalid", "captureFailed", "conflict"];

/** A file value: a string sets, `false` unsets. */
export type FileValue = string | false;
export type Table = "env" | "agentClaudeEnv";

export interface Warning {
  key: string;
  message: string;
}

export interface HostSettings {
  hostId: string;
  label: string;
  remote: boolean;
  path: string;
  revision: number;
  loadedRevision: number;
  fileStatus: { kind: "ok" } | { kind: "invalid"; message: string; line: number; col: number };
  file: {
    env: Record<string, FileValue>;
    agentClaudeEnv: Record<string, FileValue>;
    command: string | null;
    shell: string | null;
    captureEnabled: boolean;
    captureTimeout: number;
    unknown: Record<string, string>;
  };
  loaded: HostSettings["file"];
  captureError: string | null;
  daemon: { version: string; pid: number; startedAt: number; socket: string; log: string; executable: string; bundled: string };
  sessions: { name: string; plane: string; busy: string | null }[];
}

const LOGIN: Record<string, string> = {
  PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
  HOME: "/Users/me",
  EDITOR: "vi",
  HTTP_PROXY: "http://proxy.corp:3128",
};
const PROTECTED = new Set(["TERM", "COLORTERM", "PWD", "BITPLANE_SESSION_ID", "CLAUDECODE"]);

export function expand(value: string, below: Record<string, string>): string {
  return value.replace(/\$\$|\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g, (m, name) =>
    m === "$$" ? "$" : (below[name] ?? ""),
  );
}

function layer(base: Record<string, string>, table: Record<string, FileValue>) {
  const out = { ...base };
  for (const [k, v] of Object.entries(table)) {
    if (PROTECTED.has(k)) continue;
    if (v === false) delete out[k];
    else out[k] = expand(v, base);
  }
  return out;
}

export interface Resolved {
  env: Record<string, string | null>;
  agentClaudeEnv: Record<string, string | null>;
  command: { path: string | null; source: "setting" | "path" };
  shell: { path: string; source: "setting" | "fallback" };
  warnings: Warning[];
}

export function resolve(file: HostSettings["file"], captureFailed: boolean): Resolved {
  const login = captureFailed ? { PATH: "/usr/bin:/bin", HOME: "/Users/me" } : LOGIN;
  const afterEnv = layer(login, file.env);
  const afterAgent = layer(afterEnv, file.agentClaudeEnv);
  const warnings: Warning[] = [];
  const pick = (table: Record<string, FileValue>, below: Record<string, string>, prefix: string) =>
    Object.fromEntries(
      Object.entries(table).map(([k, v]) => {
        if (PROTECTED.has(k)) {
          warnings.push({ key: `${prefix}${k}`, message: `${k} is set by the daemon; ignored` });
          return [k, null];
        }
        if (v === false) return [k, null];
        for (const [, name] of v.matchAll(/\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g))
          if (!(name in below))
            warnings.push({ key: `${prefix}${k}`, message: `\${${name}} is not defined; expands to ""` });
        return [k, expand(v, below)];
      }),
    );
  const env = pick(file.env, login, "env.");
  const agentClaudeEnv = pick(file.agentClaudeEnv, afterEnv, "agent.claude.env.");
  for (const key of Object.keys(file.unknown))
    warnings.push({ key, message: "not known to this daemon; ignored" });
  if (file.command && !file.command.startsWith("/"))
    warnings.push({ key: "agent.claude.command", message: "must be an absolute path; looking claude up on PATH" });
  const agentPath = afterAgent.PATH ?? "";
  return {
    env,
    agentClaudeEnv,
    command:
      file.command && file.command.startsWith("/")
        ? { path: expand(file.command, afterEnv), source: "setting" }
        : { path: agentPath.includes("/opt/homebrew/bin") ? "/opt/homebrew/bin/claude" : "/Users/me/.local/bin/claude", source: "path" },
    shell: file.shell ? { path: file.shell, source: "setting" } : { path: "/bin/zsh", source: "fallback" },
    warnings,
  };
}

function baseFile(): HostSettings["file"] {
  return {
    env: {
      PATH: "/opt/homebrew/bin:${PATH}",
      EDITOR: "nvim",
      HTTP_PROXY: false,
    },
    agentClaudeEnv: {
      CLAUDE_CONFIG_DIR: "${HOME}/.claude-work",
      ANTHROPIC_MODEL: "claude-opus-5-5",
    },
    command: null,
    shell: "/opt/homebrew/bin/fish",
    captureEnabled: true,
    captureTimeout: 10,
    unknown: { "scrollback.max_bytes": "10485760" },
  };
}

function hosts(scenario: Scenario): HostSettings[] {
  const loaded = baseFile();
  const file = structuredClone(loaded);
  if (scenario === "pending") {
    file.env.TERM = "screen-256color";
    file.env.GOPATH = "${GOROOT}/work";
    file.agentClaudeEnv.ANTHROPIC_MODEL = "claude-sonnet-5-5";
  }
  return [
    {
      hostId: "local",
      label: "MacBook (local)",
      remote: false,
      path: "~/.config/bitplane/sessiond/settings.toml",
      revision: scenario === "pending" ? 2 : 1,
      loadedRevision: 1,
      fileStatus:
        scenario === "invalid"
          ? { kind: "invalid", message: 'unclosed "${" in env.GOPATH', line: 7, col: 12 }
          : { kind: "ok" },
      file,
      loaded,
      captureError:
        scenario === "captureFailed" ? "/opt/homebrew/bin/fish: timed out after 10s" : null,
      daemon: {
        version: "0.4.2",
        pid: 48213,
        startedAt: Date.now() - (3 * 60 + 12) * 60_000,
        socket: "~/Library/Application Support/bitplane/sessiond.sock",
        log: "~/Library/Logs/bitplane/sessiond.log",
        executable: "/Applications/cc.app/Contents/MacOS/bitplane-sessiond",
        bundled: "/Applications/cc.app/Contents/MacOS/bitplane-sessiond",
      },
      sessions: [
        { name: "zsh", plane: "alpha", busy: null },
        { name: "Claude · refactor auth", plane: "alpha", busy: "busy" },
        { name: "Claude · docs", plane: "beta", busy: "waiting for permission" },
        { name: "vim", plane: "beta", busy: "vim running" },
      ],
    },
    {
      hostId: "build-box",
      label: "build-box",
      remote: true,
      path: "~/.config/bitplane/sessiond/settings.toml",
      revision: 1,
      loadedRevision: 1,
      fileStatus: { kind: "ok" },
      file: { ...baseFile(), env: { PATH: "${HOME}/.cargo/bin:${PATH}" }, shell: null, unknown: {} },
      loaded: { ...baseFile(), env: { PATH: "${HOME}/.cargo/bin:${PATH}" }, shell: null, unknown: {} },
      captureError: null,
      daemon: {
        version: "0.4.1",
        pid: 902,
        startedAt: Date.now() - 6 * 24 * 60 * 60_000,
        socket: "ssh build-box → ~/.local/state/bitplane/sessiond.sock",
        log: "~/.local/state/bitplane/sessiond.log",
        executable: "~/.local/bin/bitplane-sessiond",
        bundled: "/Applications/cc.app/Contents/MacOS/bitplane-sessiond",
      },
      sessions: [{ name: "cargo build", plane: "gamma", busy: "cargo running" }],
    },
  ];
}

interface Store {
  scenario: Scenario;
  hosts: HostSettings[];
  accent: string;
  conflictArmed: boolean;
  log: string[];
}

let state: Store = { scenario: "clean", hosts: hosts("clean"), accent: "#4cb8b4", conflictArmed: false, log: [] };
const listeners = new Set<() => void>();
function set(next: Partial<Store>) {
  state = { ...state, ...next };
  for (const l of listeners) l();
}
export function useSettings(): Store {
  return useSyncExternalStore(
    (l) => (listeners.add(l), () => listeners.delete(l)),
    () => state,
  );
}

export function setScenario(scenario: Scenario) {
  set({ scenario, hosts: hosts(scenario), conflictArmed: scenario === "conflict", log: [] });
}

export function setAccent(accent: string) {
  document.documentElement.style.setProperty("--accent", accent);
  document.documentElement.style.setProperty("--accent-soft", `${accent}24`);
  set({ accent, log: [...state.log, `cc settings.toml: accent = "${accent}"`] });
}

export type Op =
  | { op: "set"; table: Table; key: string; value: FileValue }
  | { op: "remove"; table: Table; key: string }
  | { op: "setScalar"; field: "command" | "shell" | "captureEnabled" | "captureTimeout"; value: string | number | boolean | null };

/** Fake `settings.set`; returns an error message or null. */
export function applyOps(hostId: string, revision: number, ops: Op[]): string | null {
  const host = state.hosts.find((h) => h.hostId === hostId);
  if (!host) return "unknown host";
  if (state.conflictArmed) {
    const changed = structuredClone(host);
    changed.revision += 1;
    changed.file.env.EDITOR = "code --wait";
    set({
      conflictArmed: false,
      hosts: state.hosts.map((h) => (h === host ? changed : h)),
      log: [...state.log, "settings.set → SettingsChanged (file edited by hand)"],
    });
    return "SettingsChanged";
  }
  if (revision !== host.revision) return "SettingsChanged";
  if (host.fileStatus.kind === "invalid") return "SettingsInvalid";
  for (const op of ops)
    if (op.op === "set" && typeof op.value === "string" && /\$\{(?![A-Za-z_][A-Za-z0-9_]*\})/.test(op.value))
      return `SettingsInvalid: bad \${ in ${op.key}`;
  const next = structuredClone(host);
  for (const op of ops) {
    if (op.op === "set") next.file[op.table][op.key] = op.value;
    else if (op.op === "remove") delete next.file[op.table][op.key];
    else (next.file as Record<string, unknown>)[op.field] = op.value;
  }
  next.revision += 1;
  set({
    hosts: state.hosts.map((h) => (h === host ? next : h)),
    log: [...state.log, `settings.set ${hostId} rev ${revision}: ${ops.map(describe).join(", ")}`],
  });
  return null;
}

function describe(op: Op): string {
  if (op.op === "set") return `${op.table}.${op.key} = ${JSON.stringify(op.value)}`;
  if (op.op === "remove") return `remove ${op.table}.${op.key}`;
  return `${op.field} = ${JSON.stringify(op.value)}`;
}

/** Fake restart: the new instance loads the file. */
export function restart(hostId: string) {
  set({
    hosts: state.hosts.map((h) =>
      h.hostId === hostId
        ? { ...h, loaded: structuredClone(h.file), loadedRevision: h.revision, captureError: null, daemon: { ...h.daemon, pid: h.daemon.pid + 17, startedAt: Date.now() } }
        : h,
    ),
    log: [...state.log, `daemon.shutdown{restore:true} ${hostId} → auto-start → new hello`],
  });
}

export function reloadFixed(hostId: string) {
  set({
    hosts: state.hosts.map((h) =>
      h.hostId === hostId ? { ...h, fileStatus: { kind: "ok" }, revision: h.revision + 1 } : h,
    ),
  });
}

export const pending = (h: HostSettings) => h.revision !== h.loadedRevision;
