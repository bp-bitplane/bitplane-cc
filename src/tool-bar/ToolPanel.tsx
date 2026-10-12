import type { KeyboardEvent, PointerEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import {
  MAX_TOOL_PANEL_WIDTH,
  MIN_TOOL_PANEL_WIDTH,
  setToolPanelWidth,
  useStore,
} from "../store/store";
import { Centered } from "../ui/Centered";
import { Files, FilesActions } from "./files/Files";
import { SourceControl, SourceControlActions } from "./SourceControl";
import styles from "./ToolPanel.module.css";
import { type ToolPanelView, toolPanelView } from "./toolPanelView";
import { TOOL_LABELS, type Tool } from "./tools";

const KEYBOARD_STEP = 16;

export function ToolPanel() {
  const open = useStore((s) => s.toolPanelOpen);
  const tool = useStore((s) => s.activeTool);
  const width = useStore((s) => s.toolPanelWidth);
  const view = useStore(useShallow(toolPanelView));
  if (!open) return null;
  return (
    <aside className={styles.panel} style={{ width }} aria-label={TOOL_LABELS[tool]}>
      <ResizeHandle width={width} />
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

/** Dragging left widens the panel: it sits left of the Tool bar. */
function ResizeHandle({ width }: { width: number }) {
  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const handle = event.currentTarget;
    const startX = event.clientX;
    handle.setPointerCapture(event.pointerId);
    const onMove = (move: globalThis.PointerEvent) =>
      setToolPanelWidth(width + startX - move.clientX);
    const onUp = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      handle.removeEventListener("pointercancel", onUp);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
    handle.addEventListener("pointercancel", onUp);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") setToolPanelWidth(width + KEYBOARD_STEP);
    else if (event.key === "ArrowRight") setToolPanelWidth(width - KEYBOARD_STEP);
  };
  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable separator is the ARIA pattern for a splitter.
    <div
      className={styles.handle}
      role="separator"
      aria-label="Resize tool panel"
      aria-orientation="vertical"
      aria-valuenow={width}
      aria-valuemin={MIN_TOOL_PANEL_WIDTH}
      aria-valuemax={MAX_TOOL_PANEL_WIDTH}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    />
  );
}
