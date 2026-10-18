import { render, screen } from "@testing-library/react";
import { selectSession, useStore } from "../store/store";
import { mockBackend, plane, planesModel, seed, session, sessionsById } from "../test/fixtures";
import { MainArea } from "./MainArea";

vi.mock("../terminal/TerminalStack", () => ({
  TerminalStack: ({ visibleId }: { visibleId: number | null }) => (
    <div data-testid="terminals" data-visible={visibleId ?? "none"} />
  ),
}));

describe("MainArea", () => {
  beforeEach(() => {
    mockBackend();
    seed({
      planes: planesModel([plane("a")]),
      sessions: sessionsById(session(1, "a")),
      fileTabs: { a: [{ planeId: "a", path: "src/foo.ts", mode: "worktree", pinned: true }] },
      selectedPlaneId: "a",
      activeTab: { a: "worktree:src/foo.ts" },
    });
  });

  it("shows the active File tab over the terminals, which stay mounted", () => {
    render(<MainArea />);
    expect(screen.getByRole("heading", { name: "foo.ts (Working Tree)" })).toBeInTheDocument();
    expect(screen.getByTestId("terminals")).toHaveAttribute("data-visible", "none");
  });

  it("shows the terminal again when a session is selected", async () => {
    render(<MainArea />);
    selectSession("a", 1);
    expect(await screen.findByTestId("terminals")).toHaveAttribute("data-visible", "1");
    expect(screen.queryByRole("heading", { name: "foo.ts (Working Tree)" })).toBeNull();
  });

  it("keeps the tab bar for a plane with only File tabs", () => {
    useStore.setState({ sessions: {} });
    render(<MainArea />);
    expect(screen.getByRole("tab", { name: "foo.ts (Working Tree)" })).toBeInTheDocument();
    expect(screen.queryByText(/No sessions in/)).toBeNull();
  });
});
