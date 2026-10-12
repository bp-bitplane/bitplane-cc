import { useShallow } from "zustand/react/shallow";
import {
  MAX_TOOL_PANEL_WIDTH,
  MIN_TOOL_PANEL_WIDTH,
  setToolPanelWidth,
  useStore,
} from "../store/store";
import { Centered } from "../ui/Centered";
import { ResizeHandle } from "../ui/ResizeHandle";
import { Files, FilesActions } from "./files/Files";
import { SourceControl, SourceControlActions } from "./SourceControl";
import styles from "./ToolPanel.module.css";
import { type ToolPanelView, toolPanelView } from "./toolPanelView";
import { TOOL_LABELS, type Tool } from "./tools";

export function ToolPanel() {
  const open = useStore((s) => s.toolPanelOpen);
  const tool = useStore((s) => s.activeTool);
  const width = useStore((s) => s.toolPanelWidth);
  const view = useStore(useShallow(toolPanelView));
  if (!open) return null;
  return (
    <aside className={styles.panel} style={{ width }} aria-label={TOOL_LABELS[tool]}>
      <ResizeHandle
        edge="left"
        label="Resize tool panel"
        width={width}
        min={MIN_TOOL_PANEL_WIDTH}
        max={MAX_TOOL_PANEL_WIDTH}
        onResize={setToolPanelWidth}
      />
      <header className={styles.head} data-tauri-drag-region>
        <span>{TOOL_LABELS[tool]}</span>
        {view.screen === null && view.planeId && <ToolActions tool={tool} planeId={view.planeId} />}
      </header>
      <div className={styles.body}>
        {view.screen === null && view.planeId ? (
          <ToolContent tool={tool} planeId={view.planeId} />
        ) : (
          <Notice view={view} />
        )}
      </div>
    </aside>
  );
}

function ToolContent({ tool, planeId }: { tool: Tool; planeId: string }) {
  switch (tool) {
    case "sourceControl":
      return <SourceControl planeId={planeId} />;
    case "files":
      return <Files planeId={planeId} />;
  }
}

function ToolActions({ tool, planeId }: { tool: Tool; planeId: string }) {
  switch (tool) {
    case "sourceControl":
      return <SourceControlActions planeId={planeId} />;
    case "files":
      return <FilesActions planeId={planeId} />;
  }
}

function Notice({ view }: { view: ToolPanelView }) {
  switch (view.screen) {
    case "empty":
      return (
        <Centered title="No plane">
          <p>Select a plane in the sidebar.</p>
        </Centered>
      );
    case "gone":
      return (
        <Centered title={`${view.planeId} is gone`}>
          <p>Its plane root no longer exists.</p>
        </Centered>
      );
    case "creating":
      return (
        <Centered title={`${view.planeId} is being created`}>
          <p>Tools are available once it's ready.</p>
        </Centered>
      );
    default:
      return null;
  }
}
