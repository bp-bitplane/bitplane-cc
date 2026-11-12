import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import {
  agent,
  mockBackend,
  plane,
  planesModel,
  seed,
  session,
  sessionsById,
} from "../test/fixtures";
import { TabBar } from "./TabBar";

describe("TabBar", () => {
  it("names tabs by position and marks the active one", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(4, "a", { title: "vim" }), session(8, "a")),
      activeTab: { a: 8 },
    });
    render(<TabBar planeId="a" />);
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.getAttribute("aria-label"))).toEqual(["1 · vim", "2 · zsh"]);
    expect(tabs.map((t) => t.textContent)).toEqual(["vim", "zsh"]);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
  });

  it("shows each tab's kind as an icon", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(
        session(1, "a"),
        session(2, "a", { agent: "claude" }),
        session(3, "a", { agent: "codex" }),
      ),
    });
    const { container } = render(<TabBar planeId="a" />);
    const icons = [...container.querySelectorAll("[data-kind]")];
    expect(icons.map((icon) => icon.getAttribute("data-kind"))).toEqual([
      "terminal",
      "claude",
      "agent",
    ]);
  });

  it("swaps the icons for tab numbers only while ⌘ is held, in the same slot", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a"), session(2, "a", { agent: "claude" })),
      agents: { now: agent("now", "key-2", { state: { kind: "busy" } }) },
    });
    const { container } = render(<TabBar planeId="a" />);
    const slots = () => screen.getAllByRole("tab").map((tab) => tab.previousElementSibling);
    const before = slots();
    expect(before.map((slot) => slot?.textContent)).toEqual(["", ""]);

    fireEvent.keyDown(window, { key: "Meta", code: "MetaLeft", metaKey: true });
    for (const [i, slot] of slots().entries()) expect(slot).toBe(before[i]);
    expect(slots().map((slot) => slot?.textContent)).toEqual(["1", "2"]);
    expect(container.querySelectorAll("[data-kind]")).toHaveLength(0);
    expect(screen.getByRole("img", { name: "Agent busy" })).toBeInTheDocument();

    fireEvent.keyUp(window, { key: "Meta", code: "MetaLeft", metaKey: false });
    expect(slots().map((slot) => slot?.textContent)).toEqual(["", ""]);
    expect(container.querySelectorAll("[data-kind]")).toHaveLength(2);

    fireEvent.keyDown(window, { key: "Meta", code: "MetaLeft", metaKey: true });
    fireEvent.blur(window);
    expect(slots().map((slot) => slot?.textContent)).toEqual(["", ""]);
    fireEvent.keyDown(window, { key: "Control", code: "ControlLeft", ctrlKey: true });
    expect(slots().map((slot) => slot?.textContent)).toEqual(["", ""]);
  });

  it("switches tabs and opens the launcher", async () => {
    const backend = mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(4, "a"), session(8, "a")),
      selectedPlaneId: "a",
    });
    render(<TabBar planeId="a" />);
    await userEvent.click(screen.getByRole("tab", { name: "2 · zsh" }));
    expect(useStore.getState().activeTab.a).toBe(8);
    await userEvent.click(screen.getByRole("button", { name: "New session" }));
    expect(useStore.getState().dialog).toEqual({ kind: "launcher", planeId: "a" });
    expect(backend.called("session_open")).toHaveLength(0);
  });

  it("shows each tab's agent state as a badge on its icon", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(
        ...[1, 2, 3, 4, 5, 6].map((id) => session(id, "a", { agent: id === 4 ? null : "claude" })),
      ),
      agents: {
        old: agent("old", "key-1", { state: { kind: "exited" } }),
        now: agent("now", "key-1", { state: { kind: "busy" } }),
        ask: agent("ask", "key-2", { state: { kind: "waiting", reason: "permission" } }),
        back: agent("back", "key-3", { state: { kind: "resuming" } }),
        closed: agent("closed", null),
        calm: agent("calm", "key-5"),
        done: agent("done", "key-6", { state: { kind: "exited" } }),
      },
    });
    render(<TabBar planeId="a" />);
    for (const badge of screen.getAllByRole("img")) {
      expect(badge.parentElement?.querySelector("[data-kind]")).not.toBeNull();
    }
    expect(screen.getAllByRole("img").map((dot) => dot.getAttribute("aria-label"))).toEqual([
      "Agent busy",
      "Agent waiting for permission",
      "Agent resuming",
      "Agent idle",
      "Agent exited",
    ]);
  });

  it("renames a tab on double-click", async () => {
    const backend = mockBackend({
      session_rename: (args) => session(1, "a", { name: args.name as string }),
    });
    seed({ planes: planesModel([plane("a")]), sessions: sessionsById(session(1, "a")) });
    render(<TabBar planeId="a" />);
    await userEvent.dblClick(screen.getByRole("tab", { name: "1 · zsh" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Rename 1 · zsh" }), "build{Enter}");
    expect(backend.called("session_rename")[0]?.args).toEqual({ id: 1, name: "build" });
    expect(await screen.findByRole("tab", { name: "1 · build" })).toBeInTheDocument();
  });

  it("keeps the name when a rename is cancelled", async () => {
    const backend = mockBackend();
    seed({ planes: planesModel([plane("a")]), sessions: sessionsById(session(1, "a")) });
    render(<TabBar planeId="a" />);
    await userEvent.dblClick(screen.getByRole("tab", { name: "1 · zsh" }));
    await userEvent.type(screen.getByRole("textbox", { name: "Rename 1 · zsh" }), "x{Escape}");
    expect(backend.called("session_rename")).toHaveLength(0);
    expect(screen.getByRole("tab", { name: "1 · zsh" })).toBeInTheDocument();
  });

  it("shows the program's title beside a named tab", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a", { name: "build", title: "vim notes" })),
    });
    render(<TabBar planeId="a" />);
    const tab = screen.getByRole("tab", { name: "1 · build" });
    expect(tab).toHaveAttribute("title", "1 · build — vim notes");
    expect(screen.getByText("vim notes")).toBeInTheDocument();
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
    expect(tabs.map((t) => t.textContent)).toEqual(["zsh", "index.ts (Index)api", "index.tsweb"]);
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
