import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import { mockBackend, plane, planesModel, seed, session, sessionsById } from "../test/fixtures";
import { TabBar } from "./TabBar";

describe("TabBar", () => {
  it("numbers tabs by position and marks the active one", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(4, "a", { title: "vim" }), session(8, "a")),
      activeTab: { a: 8 },
    });
    render(<TabBar planeId="a" />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["1 · vim", "2 · zsh"]);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
  });

  it("switches tabs and opens new sessions", async () => {
    const backend = mockBackend({ session_open: () => session(9, "a") });
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(4, "a"), session(8, "a")),
      selectedPlaneId: "a",
    });
    render(<TabBar planeId="a" />);
    await userEvent.click(screen.getByRole("tab", { name: "2 · zsh" }));
    expect(useStore.getState().activeTab.a).toBe(8);
    await userEvent.click(screen.getByRole("button", { name: "New session" }));
    expect(backend.called("session_open")).toHaveLength(1);
  });

  it("disables new sessions on a gone plane", () => {
    mockBackend();
    seed({ planes: planesModel([plane("a", "gone")]), sessions: sessionsById(session(1, "a")) });
    render(<TabBar planeId="a" />);
    expect(screen.getByRole("button", { name: "New session" })).toBeDisabled();
  });

  it("asks the backend whether a session is busy before closing it", async () => {
    const backend = mockBackend({ session_busy: () => ({ busy: false, processName: null }) });
    seed({ planes: planesModel([plane("a")]), sessions: sessionsById(session(1, "a")) });
    render(<TabBar planeId="a" />);
    await userEvent.click(screen.getByRole("button", { name: "Close 1 · zsh" }));
    expect(backend.called("session_busy")).toHaveLength(1);
    expect(backend.called("session_close")[0]?.args).toEqual({ id: 1 });
  });

  it("lists File tabs after the sessions, unnumbered, with their folder", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a")),
      fileTabs: {
        a: [
          { planeId: "a", path: "api/index.ts", mode: "index", pinned: true },
          { planeId: "a", path: "web/index.ts", mode: "file", pinned: false },
        ],
      },
      activeTab: { a: "file:web/index.ts" },
    });
    render(<TabBar planeId="a" />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual([
      "1 · zsh",
      "index.ts (Index)api",
      "index.tsweb",
    ]);
    expect(tabs[1]).toHaveAttribute("title", "a/api");
    expect(tabs[2]).toHaveAttribute("aria-selected", "true");
  });

  it("selects, pins and closes File tabs", async () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a")),
      fileTabs: { a: [{ planeId: "a", path: "foo.ts", mode: "file", pinned: false }] },
      selectedPlaneId: "a",
    });
    render(<TabBar planeId="a" />);
    const tab = screen.getByRole("tab", { name: "foo.ts" });
    await userEvent.click(tab);
    expect(useStore.getState().activeTab.a).toBe("file:foo.ts");
    await userEvent.dblClick(tab);
    expect(useStore.getState().fileTabs.a?.[0]?.pinned).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "Close foo.ts" }));
    expect(useStore.getState().fileTabs.a).toEqual([]);
    expect(useStore.getState().activeTab.a).toBe(1);
  });
});
