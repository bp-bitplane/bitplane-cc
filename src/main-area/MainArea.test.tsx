import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { openFileTab, selectSession, useStore } from "../store/store";
import {
  agent,
  mockBackend,
  plane,
  planesModel,
  seed,
  session,
  sessionsById,
} from "../test/fixtures";
import { MainArea } from "./MainArea";

vi.mock("../file-tab/DiffView", () => ({
  default: ({ kind }: { kind: string }) => <div data-testid="diff-view">{kind}</div>,
  highlightCode: async () => null,
}));

vi.mock("../terminal/TerminalStack", () => ({
  TerminalStack: ({ visibleId }: { visibleId: number | null }) => (
    <div data-testid="terminals" data-visible={visibleId ?? "none"} />
  ),
}));

describe("MainArea", () => {
  beforeEach(() => {
    mockBackend({ plane_file_diff: () => ({ kind: "diff", patch: "" }) });
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a")),
      fileTabs: { a: [{ planeId: "a", path: "src/foo.ts", mode: "worktree", pinned: true }] },
      selectedPlaneId: "a",
      activeTab: { a: "worktree:src/foo.ts" },
    });
  });

  it("shows the active File tab over the terminals, which stay mounted", async () => {
    render(<MainArea />);
    expect(await screen.findByTestId("diff-view")).toBeInTheDocument();
    expect(screen.getByTestId("terminals")).toHaveAttribute("data-visible", "none");
  });

  it("shows the terminal again when a session is selected", async () => {
    render(<MainArea />);
    selectSession("a", 1);
    expect(await screen.findByTestId("terminals")).toHaveAttribute("data-visible", "1");
    expect(screen.queryByTestId("diff-view")).toBeNull();
  });

  it("shows the shown tab's agent errors above the terminal", () => {
    useStore.setState({
      activeTab: { a: 1 },
      agents: {
        x: agent("x", "key-1", {
          polled: true,
          lastError: { error: "rate_limit", message: "Rate limit reached" },
        }),
      },
    });
    render(<MainArea />);
    const alerts = screen.getAllByRole("alert").map((a) => a.textContent);
    expect(alerts).toEqual([
      "Agent hooks didn't load; your shell may be overriding PATH. This agent's state is polled and may lag a few seconds.",
      "The agent's last turn failed (rate_limit): Rate limit reached",
    ]);

    act(() =>
      useStore.setState({ agents: { x: agent("x", "key-1", { state: { kind: "busy" } }) } }),
    );
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps the tab bar for a plane with only File tabs", () => {
    useStore.setState({ sessions: {} });
    render(<MainArea />);
    expect(screen.getByRole("tab", { name: "foo.ts (Working Tree)" })).toBeInTheDocument();
    expect(screen.queryByText(/No sessions in/)).toBeNull();
  });

  it("keeps a File tab's Markdown preview across switching tabs", async () => {
    mockBackend({
      plane_file_diff: () => ({ kind: "plain" }),
      plane_read_file: () => ({ kind: "text", contents: "# Hello" }),
    });
    render(<MainArea />);
    act(() => openFileTab("a", "README.md", "file"));
    await screen.findByRole("heading", { name: "Hello" });
    await userEvent.click(screen.getByRole("button", { name: "Source" }));
    expect(screen.getByTestId("diff-view")).toBeInTheDocument();
    act(() => selectSession("a", 1));
    expect(screen.queryByTestId("diff-view")).toBeNull();
    act(() => openFileTab("a", "README.md", "file"));
    expect(await screen.findByTestId("diff-view")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute("aria-pressed", "true");
  });
});

describe("session daemon banners", () => {
  it("shows a failed start with its log and retries", async () => {
    const backend = mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      daemon: {
        kind: "failed",
        message: "the daemon did not answer within 5s",
        log: "/s/sessiond.log",
      },
    });
    render(<MainArea />);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Couldn't start the session daemon: the daemon did not answer within 5s Log: /s/sessiond.log",
    );
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(backend.called("daemon_retry")).toHaveLength(1);
  });

  it("names each unavailable module with the daemon's reason", () => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      daemon: {
        kind: "connected",
        version: "0.1.0",
        instance: "i1",
        restarted: false,
        modules: [
          {
            module: "plane",
            available: false,
            reason: "bp isn't on your PATH; install bitplane-cli",
          },
          { module: "git", available: true, reason: null },
        ],
      },
    });
    render(<MainArea />);
    const alerts = screen.getAllByRole("alert");
    expect(alerts).toHaveLength(1);
    expect(alerts[0]).toHaveTextContent(
      "plane unavailable: bp isn't on your PATH; install bitplane-cli",
    );
  });
});
