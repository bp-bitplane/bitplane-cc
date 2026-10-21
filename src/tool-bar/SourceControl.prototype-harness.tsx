/**
 * PROTOTYPE (#36): a browser-only harness for the Source control variants.
 * Renders the real Sidebar and Tool panel next to each other with a mocked backend,
 * so `pnpm prototype:sc` shows the variants without Tauri or real repos.
 */
import "../theme.css";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createRoot } from "react-dom/client";
import type { Repo } from "../ipc";
import { Sidebar } from "../sidebar/Sidebar";
import { initialState, useStore } from "../store/store";
import { ToolPanel } from "./ToolPanel";

const REPOS: Repo[] = [
  {
    name: "bitplane-cc",
    path: "/planes/alpha/projects/bitplane-cc",
    state: {
      kind: "ready",
      status: {
        head: { kind: "branch", name: "alfonz/source-control-readability" },
        tracking: { ahead: 2, behind: 1 },
        staged: [
          { path: "src/tool-bar/SourceControl.tsx", kind: "modified", from: null },
          { path: "src/tool-bar/sourceControlPrototype.tsx", kind: "added", from: null },
        ],
        unstaged: [
          { path: "src/tool-bar/SourceControl.module.css", kind: "modified", from: null },
          { path: "src/sidebar/Sidebar.module.css", kind: "modified", from: null },
          { path: "src/store/store.ts", kind: "modified", from: null },
          { path: "docs/adr/0010-old-decision.md", kind: "deleted", from: null },
          { path: "src/ui/Badges.tsx", kind: "renamed", from: "src/ui/PlaneBadges.tsx" },
          { path: "README.md", kind: "conflicted", from: null },
        ],
        untracked: [
          "src/tool-bar/files/very-long-file-name-for-truncation-check.test.ts",
          "notes/",
        ],
      },
    },
  },
  {
    name: "bitplane",
    path: "/planes/alpha/projects/bitplane",
    state: {
      kind: "ready",
      status: {
        head: { kind: "branch", name: "main" },
        tracking: { ahead: 0, behind: 0 },
        staged: [],
        unstaged: [
          { path: "cmd/bp/create.go", kind: "modified", from: null },
          { path: "internal/plane/members.go", kind: "modified", from: null },
        ],
        untracked: [],
      },
    },
  },
  {
    name: "bitplane-protocol",
    path: "/planes/alpha/projects/bitplane-protocol",
    state: {
      kind: "ready",
      status: {
        head: { kind: "detached", sha: "0123456" },
        tracking: null,
        staged: [],
        unstaged: [],
        untracked: [],
      },
    },
  },
  { name: "infra", path: "/planes/alpha/projects/infra", state: { kind: "missing" } },
];

mockIPC((cmd) => (cmd === "plane_source_control" ? REPOS : null), { shouldMockEvents: true });

const plane = (id: string) => ({
  id,
  directory: `/planes/${id}`,
  kind: "plane" as const,
  findings: [],
  members: [],
});
const session = (id: number, planeId: string, title: string) => ({
  id,
  key: `key-${id}`,
  planeId,
  seq: id,
  status: { kind: "running" as const },
  title,
  shellName: "zsh",
});

useStore.setState(
  {
    ...initialState(),
    planes: {
      status: { kind: "ready" },
      planesDir: "/planes",
      planesDirExists: true,
      refreshedAt: Date.now(),
      envWarning: null,
      planes: [plane("alpha"), plane("beta"), plane("gamma")],
    },
    sessions: {
      1: session(1, "alpha", "claude"),
      2: session(2, "alpha", "pnpm dev"),
      3: session(3, "beta", "zsh"),
    },
    selectedPlaneId: "alpha",
    activeTab: { alpha: "worktree:projects/bitplane-cc/src/store/store.ts" },
    fileTabs: {
      alpha: [
        {
          planeId: "alpha",
          path: "projects/bitplane-cc/src/store/store.ts",
          mode: "worktree",
          pinned: false,
        },
      ],
    },
    toolPanelOpen: true,
    activeTool: "sourceControl",
  },
  true,
);

function Harness() {
  return (
    <div style={{ display: "flex", height: "100%" }}>
      <Sidebar />
      <main style={{ flex: 1, display: "grid", placeItems: "center", color: "var(--faint)" }}>
        main area (prototype harness)
      </main>
      <ToolPanel />
    </div>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing");
createRoot(root).render(<Harness />);
