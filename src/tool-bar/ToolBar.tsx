import { Files, GitBranch, type LucideIcon, SquareCode } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { labelFor } from "../keymap";
import { openInVscode, selectTool, useStore } from "../store/store";
import styles from "./ToolBar.module.css";
import { toolPanelView } from "./toolPanelView";
import { TOOL_LABELS, TOOLS, type Tool } from "./tools";

const TOOL_ICONS: Record<Tool, LucideIcon> = {
  sourceControl: GitBranch,
  files: Files,
};

export function ToolBar() {
  const { planeId, screen } = useStore(useShallow(toolPanelView));
  return (
    <nav className={styles.bar} aria-label="Tools">
      <div className={styles.head} data-tauri-drag-region />
      <div className={styles.tools}>
        {TOOLS.map((tool) => (
          <ToolButton key={tool} tool={tool} />
        ))}
      </div>
      <div className={styles.pinned}>
        <button
          type="button"
          className={styles.tool}
          title="Open in VS Code"
          aria-label="Open in VS Code"
          disabled={planeId === null || screen !== null}
          onClick={() => planeId && void openInVscode(planeId)}
        >
          <SquareCode size={18} strokeWidth={1.75} />
        </button>
      </div>
    </nav>
  );
}

function ToolButton({ tool }: { tool: Tool }) {
  const platform = useStore((s) => s.platform);
  const active = useStore((s) => s.toolPanelOpen && s.activeTool === tool);
  const Icon = TOOL_ICONS[tool];
  return (
    <button
      type="button"
      className={`${styles.tool} ${active ? styles.active : ""}`}
      title={`${TOOL_LABELS[tool]} (${labelFor(platform, tool)})`}
      aria-label={TOOL_LABELS[tool]}
      aria-pressed={active}
      onClick={() => selectTool(tool)}
    >
      <Icon size={18} strokeWidth={1.75} />
    </button>
  );
}
