import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import {
  flush,
  mockBackend,
  plane,
  planesModel,
  seed,
  session,
  sessionsById,
} from "../test/fixtures";
import { Sidebar } from "./Sidebar";

const rows = [
  plane("alpha", "plane", ["member_worktree_missing"]),
  plane("beta"),
  plane("old", "gone"),
  plane("scratch", "draft"),
];

describe("Sidebar", () => {
  beforeEach(() => {
    seed({
      planes: planesModel(rows),
      sessions: sessionsById(
        session(1, "alpha", { title: "claude" }),
        session(2, "alpha"),
        session(3, "old"),
      ),
      selectedPlaneId: "alpha",
      activeTab: { alpha: 2 },
    });
  });

  it("lists planes with their badges", () => {
    mockBackend();
    render(<Sidebar />);
    const nav = screen.getByRole("navigation", { name: "Planes" });
    expect(within(nav).getByRole("button", { name: /^alpha/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(within(nav).getByRole("img", { name: "findings" })).toBeInTheDocument();
    expect(within(nav).getByText("gone")).toBeInTheDocument();
    expect(within(nav).getByText("draft")).toBeInTheDocument();
  });

  it("nests sessions under expanded planes, labelled like tabs", () => {
    mockBackend();
    render(<Sidebar />);
    expect(screen.getByRole("button", { name: "1 · claude" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "2 · zsh" })).toBeInTheDocument();
  });

  it("shows the session count only while collapsed", async () => {
    mockBackend();
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: "Collapse alpha" }));
    expect(screen.queryByRole("button", { name: "1 · claude" })).not.toBeInTheDocument();
    expect(screen.getByTitle("2 open sessions")).toBeInTheDocument();
    expect(useStore.getState().selectedPlaneId).toBe("alpha");
  });

  it("collapses and expands everything", async () => {
    mockBackend();
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(screen.queryByRole("button", { name: /^\d · zsh$/ })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Expand all" }));
    expect(screen.getAllByRole("button", { name: /^\d · zsh$/ })).toHaveLength(2);
  });

  it("selects a session and its plane", async () => {
    mockBackend();
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: "1 · zsh" }));
    expect(useStore.getState().selectedPlaneId).toBe("old");
    expect(useStore.getState().activeTab.old).toBe(3);
  });

  it("opens a session when selecting a plane without any", async () => {
    const backend = mockBackend({ session_open: () => session(9, "beta") });
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: "beta" }));
    await flush();
    expect(backend.called("session_open")[0]?.args).toEqual({ planeId: "beta" });
  });

  it("refreshes from the header button", async () => {
    const backend = mockBackend();
    render(<Sidebar />);
    await userEvent.click(screen.getByRole("button", { name: /Refresh/ }));
    expect(backend.called("refresh")).toHaveLength(1);
  });

  it("shows the planes directory and the environment warning", () => {
    mockBackend();
    seed({ planes: planesModel([], { envWarning: "Couldn't read your shell environment" }) });
    render(<Sidebar />);
    expect(screen.getByText(/\/planes/)).toBeInTheDocument();
    expect(screen.getByText("Couldn't read your shell environment")).toBeInTheDocument();
  });
});
