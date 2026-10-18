import { mockBackend, planesModel, seed, session, sessionsById } from "../test/fixtures";
import { loadUiState, saveUiStateOnChange, useStore, zoom } from "./store";
import { fromSaved, SAVE_DELAY_MS, toSaved } from "./uiState";

describe("UI state", () => {
  it("saves tabs by session key", () => {
    seed({
      planes: planesModel([]),
      sessions: sessionsById(session(1, "alpha", { key: "k1" })),
      activeTab: { alpha: 1, gone: 9 },
      selectedPlaneId: "alpha",
    });
    expect(toSaved(useStore.getState())).toEqual({
      selectedPlaneId: "alpha",
      activeTab: { alpha: "k1" },
      collapsed: {},
      fontSize: 12,
      toolPanel: { open: false, tool: "sourceControl", width: 320 },
      sidebar: { width: 270 },
      fileTab: { diffStyle: "unified" },
    });
  });

  it("restores the tool panel and clamps its width", () => {
    seed();
    loadUiState(JSON.stringify({ toolPanel: { open: true, tool: "sourceControl", width: 9999 } }));
    const state = useStore.getState();
    expect([state.toolPanelOpen, state.activeTool, state.toolPanelWidth]).toEqual([
      true,
      "sourceControl",
      640,
    ]);
  });

  it("restores the diff layout and ignores an unknown one", () => {
    seed();
    loadUiState(JSON.stringify({ fileTab: { diffStyle: "split" } }));
    expect(useStore.getState().diffStyle).toBe("split");
    expect(fromSaved(JSON.stringify({ fileTab: { diffStyle: "wide" } }), {})).not.toHaveProperty(
      "diffStyle",
    );
  });

  it("restores the sidebar and clamps its width", () => {
    seed();
    loadUiState(JSON.stringify({ sidebar: { width: 10 } }));
    expect(useStore.getState().sidebarWidth).toBe(200);
  });

  it("ignores an unknown tool", () => {
    expect(fromSaved(JSON.stringify({ toolPanel: { tool: "chat" } }), {})).not.toHaveProperty(
      "activeTool",
    );
  });

  it("saves an active File tab and restores the plane's last session instead", () => {
    seed({
      sessions: sessionsById(session(1, "alpha")),
      activeTab: { alpha: "file:src/foo.ts" },
      fileTabs: { alpha: [{ planeId: "alpha", path: "src/foo.ts", mode: "file", pinned: true }] },
    });
    const saved = toSaved(useStore.getState());
    expect(saved.activeTab).toEqual({ alpha: { file: "file:src/foo.ts" } });
    const sessions = sessionsById(session(3, "alpha"), session(5, "alpha"), session(7, "beta"));
    expect(fromSaved(JSON.stringify(saved), sessions).activeTab).toEqual({ alpha: 5 });
  });

  it("drops tabs whose key now belongs to another plane", () => {
    const sessions = sessionsById(session(1, "beta", { key: "k1" }));
    expect(fromSaved(JSON.stringify({ activeTab: { alpha: "k1" } }), sessions).activeTab).toEqual(
      {},
    );
  });

  it("saves once after a burst of changes", async () => {
    vi.useFakeTimers();
    try {
      const backend = mockBackend();
      seed();
      const stop = saveUiStateOnChange();
      zoom(1);
      zoom(1);
      await vi.advanceTimersByTimeAsync(SAVE_DELAY_MS);
      stop();
      const saves = backend.called("ui_state_save");
      expect(saves).toHaveLength(1);
      expect(JSON.parse(String(saves[0]?.args.json)).fontSize).toBe(14);
    } finally {
      vi.useRealTimers();
    }
  });
});
