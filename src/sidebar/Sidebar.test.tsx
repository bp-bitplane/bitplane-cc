import { fireEvent, render, screen, within } from "@testing-library/react";
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

const menu = vi.hoisted(() => ({
  items: [] as { text: string; action: () => void }[],
  popup: vi.fn(),
}));

vi.mock("@tauri-apps/api/menu", () => ({
  MenuItem: {
    new: async (item: (typeof menu.items)[number]) => item,
  },
  Menu: {
    new: async (options: { items: typeof menu.items }) => {
      menu.items = options.items;
      return { popup: menu.popup };
    },
  },
}));

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

  it("leaves the header's left side to the macOS traffic lights", () => {
    mockBackend();
    render(<Sidebar />);
    expect(within(screen.getByRole("banner")).queryByText("Planes")).not.toBeInTheDocument();
  });

  it("labels the header on Linux", () => {
    mockBackend();
    useStore.setState({ platform: "linux" });
    render(<Sidebar />);
    expect(within(screen.getByRole("banner")).getByText("Planes")).toBeInTheDocument();
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

  describe("context menu", () => {
    beforeEach(() => {
      menu.popup.mockClear();
    });

    it("duplicates a plane", async () => {
      mockBackend();
      render(<Sidebar />);
      fireEvent.contextMenu(screen.getByRole("button", { name: /^beta/ }));
      await flush();
      expect(menu.popup).toHaveBeenCalledOnce();
      expect(menu.items.map((item) => item.text)).toEqual(["Duplicate plane…"]);
      menu.items[0]?.action();
      expect(useStore.getState().dialog).toMatchObject({ kind: "create", duplicateOf: "beta" });
      expect(useStore.getState().selectedPlaneId).toBe("alpha");

      fireEvent.contextMenu(screen.getByRole("button", { name: /^alpha/ }));
      await flush();
      menu.items[0]?.action();
      expect(useStore.getState().dialog).toMatchObject({ duplicateOf: "alpha" });
    });

    it("is not offered for drafts or gone planes", async () => {
      mockBackend();
      render(<Sidebar />);
      fireEvent.contextMenu(screen.getByRole("button", { name: /^scratch/ }));
      fireEvent.contextMenu(screen.getByRole("button", { name: /^old/ }));
      await flush();
      expect(menu.popup).not.toHaveBeenCalled();
    });
  });
});
