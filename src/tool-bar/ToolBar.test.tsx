import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { selectTool, useStore } from "../store/store";
import { flush, mockBackend, plane, planesModel, seed } from "../test/fixtures";
import { run } from "../useShortcuts";
import { ToolBar } from "./ToolBar";
import { ToolPanel } from "./ToolPanel";

const rows = [plane("alpha"), plane("old", "gone"), plane("new", "creating")];

describe("Tool bar", () => {
  it("opens the shown plane in VS Code", async () => {
    const backend = mockBackend();
    seed({ planes: planesModel(rows), selectedPlaneId: "alpha" });
    render(<ToolBar />);
    await userEvent.click(screen.getByRole("button", { name: "Open in VS Code" }));
    expect(backend.called("plane_open_in_vscode")).toEqual([
      { cmd: "plane_open_in_vscode", args: { planeId: "alpha" } },
    ]);
  });

  it("shows why VS Code didn't open", async () => {
    mockBackend({
      plane_open_in_vscode: () => {
        throw "code isn't on your PATH";
      },
    });
    seed({ planes: planesModel(rows), selectedPlaneId: "alpha" });
    render(<ToolBar />);
    await userEvent.click(screen.getByRole("button", { name: "Open in VS Code" }));
    await flush();
    expect(useStore.getState().notice).toBe("code isn't on your PATH");
  });

  it.each(["old", "new", null])("disables Open in VS Code for %s", (planeId) => {
    mockBackend();
    seed({ planes: planesModel(rows), selectedPlaneId: planeId });
    render(<ToolBar />);
    expect(screen.getByRole("button", { name: "Open in VS Code" })).toBeDisabled();
  });
});

describe("Tool panel", () => {
  beforeEach(() => {
    mockBackend();
    seed({ planes: planesModel(rows), selectedPlaneId: "alpha" });
  });

  it("opens a tool and collapses when the active tool is clicked again", () => {
    selectTool("sourceControl");
    expect(useStore.getState().toolPanelOpen).toBe(true);
    selectTool("sourceControl");
    expect(useStore.getState().toolPanelOpen).toBe(false);
  });

  it("shows and hides with the shortcut, keeping the active tool", () => {
    render(<ToolPanel />);
    expect(screen.queryByRole("complementary")).toBeNull();
    act(() => run({ action: "toolPanel" }));
    expect(screen.getByRole("complementary", { name: "Source control" })).toBeInTheDocument();
    act(() => run({ action: "toolPanel" }));
    expect(screen.queryByRole("complementary")).toBeNull();
  });

  it("shows a notice instead of tools for a gone plane", () => {
    seed({ planes: planesModel(rows), selectedPlaneId: "old", toolPanelOpen: true });
    render(<ToolPanel />);
    expect(screen.getByText("old is gone")).toBeInTheDocument();
  });

  it("resizes by dragging and from the keyboard, within bounds", () => {
    seed({ toolPanelOpen: true, toolPanelWidth: 320 });
    render(<ToolPanel />);
    const handle = screen.getByRole("separator", { name: "Resize tool panel" });
    handle.setPointerCapture = () => undefined;
    fireEvent.pointerDown(handle, { clientX: 500, pointerId: 1 });
    fireEvent.pointerMove(handle, { clientX: 450, pointerId: 1 });
    expect(useStore.getState().toolPanelWidth).toBe(370);
    fireEvent.pointerMove(handle, { clientX: -1000, pointerId: 1 });
    expect(useStore.getState().toolPanelWidth).toBe(640);
    fireEvent.pointerUp(handle, { pointerId: 1 });
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(useStore.getState().toolPanelWidth).toBe(624);
  });
});
