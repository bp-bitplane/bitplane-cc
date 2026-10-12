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

  it("ignores an unknown tool", () => {
    expect(fromSaved(JSON.stringify({ toolPanel: { tool: "chat" } }), {})).not.toHaveProperty(
      "activeTool",
    );
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
