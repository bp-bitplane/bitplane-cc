import { SquareCode } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { openInVscode, useStore } from "../store/store";
import styles from "./ToolBar.module.css";
import { toolPanelView } from "./toolPanelView";

export function ToolBar() {
  const { planeId, screen } = useStore(useShallow(toolPanelView));
  return (
    <nav className={styles.bar} aria-label="Tools">
      <div className={styles.head} data-tauri-drag-region />
      <div className={styles.tools} />
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
