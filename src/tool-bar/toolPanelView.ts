import { findPlane, shownPlaneId } from "../store/selectors";
import type { State } from "../store/store";

export type ToolPanelScreen = "empty" | "gone" | "creating";

export interface ToolPanelView {
  planeId: string | null;
  /** Shown instead of tool content; `null` when the tool can show the plane. */
  screen: ToolPanelScreen | null;
}

export function toolPanelView(state: State): ToolPanelView {
  const planeId = shownPlaneId(state);
  if (planeId === null) return { planeId, screen: "empty" };
  const plane = findPlane(state, planeId);
  if (plane?.kind === "creating" || (!plane && state.creates[planeId])) {
    return { planeId, screen: "creating" };
  }
  if (plane?.kind === "gone") return { planeId, screen: "gone" };
  if (!plane) {
    return { planeId, screen: state.planes.status.kind === "ready" ? "gone" : "empty" };
  }
  return { planeId, screen: null };
}
