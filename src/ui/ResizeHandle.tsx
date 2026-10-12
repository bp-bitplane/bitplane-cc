import type { KeyboardEvent, PointerEvent } from "react";
import styles from "./ResizeHandle.module.css";

const KEYBOARD_STEP = 16;

/** Sits on the panel's `edge`; dragging or arrowing away from the panel widens it. */
export function ResizeHandle({
  edge,
  label,
  width,
  min,
  max,
  onResize,
}: {
  edge: "left" | "right";
  label: string;
  width: number;
  min: number;
  max: number;
  onResize: (width: number) => void;
}) {
  const outward = edge === "left" ? -1 : 1;
  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const handle = event.currentTarget;
    const startX = event.clientX;
    handle.setPointerCapture(event.pointerId);
    const onMove = (move: globalThis.PointerEvent) =>
      onResize(width + outward * (move.clientX - startX));
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
    if (event.key === "ArrowLeft") onResize(width - outward * KEYBOARD_STEP);
    else if (event.key === "ArrowRight") onResize(width + outward * KEYBOARD_STEP);
  };
  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable separator is the ARIA pattern for a splitter.
    <div
      className={`${styles.handle} ${styles[edge]}`}
      role="separator"
      aria-label={label}
      aria-orientation="vertical"
      aria-valuenow={width}
      aria-valuemin={min}
      aria-valuemax={max}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    />
  );
}
