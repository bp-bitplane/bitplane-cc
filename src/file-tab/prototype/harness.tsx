/**
 * PROTOTYPE (#37), throwaway: a browser-only page (`pnpm prototype:md`) that
 * renders the real `FileTabView` against a mocked backend, so the Markdown
 * preview variants can be judged without Tauri. `?file=` picks the scenario,
 * `?md=` the variant (the bottom bar switches it too).
 */
import "../../theme.css";
import { mockIPC } from "@tauri-apps/api/mocks";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { useStore } from "../../store/store";
import type { FileTab } from "../../tabs/fileTabs";
import { FileTabView } from "../FileTabView";
import styles from "./harness.module.css";
import { MdVariantSwitcher } from "./MdVariantSwitcher";
import README from "./samples/README.md?raw";
import README_OLD from "./samples/README.old.md?raw";
import README_PATCH from "./samples/README.patch?raw";

const SCENARIOS = {
  readme: { label: "README.md (no changes, plain)", tab: tab("README.md", "file") },
  "readme-diff": { label: "README.md (Working Tree diff)", tab: tab("README.md", "worktree") },
  ts: { label: "src/main.ts (not Markdown)", tab: tab("src/main.ts", "file") },
} as const;

type Scenario = keyof typeof SCENARIOS;

const MAIN_TS = `import { createRoot } from "react-dom/client";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing from index.html");
createRoot(root).render(<App />);
`;

mockBackend();
useStore.setState((s) => ({
  diffStyle: "split",
  planes: {
    ...s.planes,
    status: { kind: "ready" },
    planes: [{ id: "demo", directory: "/planes/demo", kind: "plane", findings: [], members: [] }],
  },
}));

function Harness() {
  const [scenario, setScenario] = useState<Scenario>(initialScenario);
  const pick = (next: Scenario) => {
    const url = new URL(window.location.href);
    url.searchParams.set("file", next);
    window.history.replaceState(null, "", url);
    setScenario(next);
  };
  const current = SCENARIOS[scenario].tab;
  return (
    <div className={styles.page}>
      <nav className={styles.tabs} aria-label="Prototype scenarios">
        <span className={styles.badge}>#37 harness</span>
        {(Object.keys(SCENARIOS) as Scenario[]).map((key) => (
          <button key={key} type="button" aria-pressed={key === scenario} onClick={() => pick(key)}>
            {SCENARIOS[key].label}
          </button>
        ))}
      </nav>
      <main className={styles.main}>
        <FileTabView key={scenario} tab={current} />
      </main>
      <MdVariantSwitcher />
    </div>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("#root is missing");
createRoot(root).render(<Harness />);

function mockBackend() {
  mockIPC((cmd, payload) => {
    const args = (payload ?? {}) as { path?: string; mode?: string };
    switch (cmd) {
      case "plane_file_diff":
        return args.path === "README.md" && args.mode === "worktree"
          ? { kind: "diff", patch: README_PATCH }
          : { kind: "plain" };
      case "plane_read_file":
        return { kind: "text", contents: args.path === "README.md" ? README : MAIN_TS };
      case "plane_file_contents":
        return { old: README_OLD, new: README };
      case "plane_open_file":
        console.info("[prototype #37] open in VS Code:", args);
        return null;
      default:
        return null;
    }
  });
}

function initialScenario(): Scenario {
  const file = new URLSearchParams(window.location.search).get("file");
  return file && file in SCENARIOS ? (file as Scenario) : "readme";
}

function tab(path: string, mode: FileTab["mode"]): FileTab {
  return { planeId: "demo", path, mode, pinned: true };
}
