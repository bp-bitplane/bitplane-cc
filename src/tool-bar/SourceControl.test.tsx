import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Repo } from "../ipc";
import { applySourceControl, useStore } from "../store/store";
import { flush, mockBackend, plane, planesModel, repo, seed } from "../test/fixtures";
import { run } from "../useShortcuts";
import { ToolBar } from "./ToolBar";
import { ToolPanel } from "./ToolPanel";

const rows = [plane("alpha"), plane("beta")];

const web = repo("web", {
  head: { kind: "branch", name: "feat/x" },
  tracking: { ahead: 2, behind: 0 },
  staged: [{ path: "src/new.ts", kind: "added", from: null }],
  unstaged: [{ path: "src/app.ts", kind: "modified", from: null }],
  untracked: ["notes.md"],
});
const api = repo("api", { head: { kind: "detached", sha: "0123456" } });
const gone: Repo = {
  name: "gone",
  path: "/planes/alpha/projects/gone",
  state: { kind: "missing" },
};

function show(repos: Repo[] | (() => Repo[]) = [web, api, gone]) {
  const backend = mockBackend({
    plane_source_control: typeof repos === "function" ? repos : () => repos,
  });
  seed({ planes: planesModel(rows), selectedPlaneId: "alpha", toolPanelOpen: true });
  render(<ToolPanel />);
  return backend;
}

function section(name: string) {
  return screen.getByRole("region", { name });
}

describe("Source control", () => {
  it("reads the shown plane when it opens", async () => {
    const backend = show();
    await flush();
    expect(backend.called("plane_source_control")).toEqual([
      { cmd: "plane_source_control", args: { planeId: "alpha" } },
    ]);
    expect(screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"))).toEqual([
      "web",
      "api",
      "gone",
    ]);
  });

  it("heads each section with the branch, ahead/behind and change count", async () => {
    show();
    await flush();
    const header = within(section("web")).getByRole("button", { name: /web/ });
    expect(header).toHaveTextContent("webfeat/x23");
    expect(within(header).getByTitle("2 ahead of upstream")).toBeInTheDocument();
    expect(within(header).queryByTitle(/behind/)).toBeNull();
    expect(within(header).getByTitle("3 changes")).toBeInTheDocument();
    expect(within(section("api")).getByRole("button", { name: /api/ })).toHaveTextContent(
      "api0123456",
    );
  });

  it("groups files as staged, unstaged and untracked", async () => {
    show();
    await flush();
    const repo = within(section("web"));
    expect(repo.getAllByRole("list").map((g) => g.getAttribute("aria-label"))).toEqual([
      "Staged",
      "Changes",
      "Untracked",
    ]);
    const group = (name: string) => within(repo.getByRole("list", { name }));
    expect(group("Staged").getByTitle("src/new.ts")).toHaveTextContent("new.tssrcA");
    expect(group("Changes").getByTitle("src/app.ts")).toHaveTextContent("app.tssrcM");
    expect(group("Untracked").getByTitle("notes.md")).toHaveTextContent("notes.mdU");
    expect(within(section("api")).getByText("No changes")).toBeInTheDocument();
  });

  it("warns about a missing worktree", async () => {
    show();
    await flush();
    expect(
      within(section("gone")).getByText(/Worktree missing: \/planes\/alpha\/projects\/gone/),
    ).toBeInTheDocument();
  });

  it("collapses sections by hand and all at once", async () => {
    show();
    await flush();
    const header = within(section("web")).getByRole("button", { name: /web/ });
    expect(header).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(header);
    expect(header).toHaveAttribute("aria-expanded", "false");
    expect(within(section("web")).queryByRole("list")).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Expand all" }));
    expect(screen.getAllByRole("button", { expanded: true })).toHaveLength(3);
    await userEvent.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(screen.getAllByRole("button", { expanded: false })).toHaveLength(3);
  });

  it("reads again on Refresh and on the refresh shortcut", async () => {
    let calls = 0;
    const backend = show(() => (++calls === 1 ? [api] : [web]));
    await flush();
    expect(screen.queryByRole("region", { name: "web" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /^Refresh/ }));
    await flush();
    expect(section("web")).toBeInTheDocument();
    await act(async () => {
      run({ action: "refresh" });
      await flush();
    });
    expect(backend.called("plane_source_control")).toHaveLength(3);
  });

  it("follows the shown plane", async () => {
    const backend = show();
    await flush();
    await act(async () => {
      useStore.setState({ selectedPlaneId: "beta" });
      await flush();
    });
    expect(backend.called("plane_source_control").at(-1)?.args).toEqual({ planeId: "beta" });
  });

  it("watches the shown plane while on screen", async () => {
    const backend = show();
    await flush();
    await act(async () => {
      useStore.setState({ selectedPlaneId: "beta" });
      await flush();
    });
    await act(async () => {
      useStore.setState({ toolPanelOpen: false });
      await flush();
    });
    expect(backend.called("plane_source_control_watch").map((c) => c.args.planeId)).toEqual([
      "alpha",
      null,
      "beta",
      null,
    ]);
  });

  it("applies pushed reads for the shown plane only", async () => {
    show([api]);
    await flush();
    act(() => applySourceControl({ planeId: "beta", repos: [web] }));
    expect(screen.queryByRole("region", { name: "web" })).toBeNull();
    act(() => applySourceControl({ planeId: "alpha", repos: [web] }));
    expect(section("web")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "api" })).toBeNull();
  });

  it("says when a plane has no repos", async () => {
    show([]);
    await flush();
    expect(screen.getByText("No repositories")).toBeInTheDocument();
  });

  it("says why git status couldn't be read", async () => {
    show(() => {
      throw "git isn't on your PATH";
    });
    await flush();
    expect(screen.getByText("Couldn't read git status")).toBeInTheDocument();
    expect(screen.getByText("git isn't on your PATH")).toBeInTheDocument();
  });

  it("opens from the Tool bar and the shortcut, and collapses when already active", async () => {
    mockBackend({ plane_source_control: () => [] });
    seed({ planes: planesModel(rows), selectedPlaneId: "alpha" });
    render(
      <>
        <ToolPanel />
        <ToolBar />
      </>,
    );
    const button = screen.getByRole("button", { name: "Source control" });
    expect(screen.getAllByRole("button")[0]).toBe(button);
    await userEvent.click(button);
    expect(button).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("complementary", { name: "Source control" })).toBeInTheDocument();
    act(() => run({ action: "sourceControl" }));
    expect(screen.queryByRole("complementary")).toBeNull();
    act(() => run({ action: "sourceControl" }));
    expect(useStore.getState().toolPanelOpen).toBe(true);
  });

  it("opens a change as a File tab in its group's mode", async () => {
    show([web]);
    await flush();
    const group = (name: string) => within(screen.getByRole("list", { name }));
    await userEvent.click(group("Staged").getByRole("button", { name: /new\.ts/ }));
    expect(useStore.getState().activeTab.alpha).toBe("index:projects/web/src/new.ts");
    await userEvent.click(group("Changes").getByRole("button", { name: /app\.ts/ }));
    expect(useStore.getState().activeTab.alpha).toBe("worktree:projects/web/src/app.ts");
    group("Untracked")
      .getByRole("button", { name: /notes\.md/ })
      .focus();
    await userEvent.keyboard("{Enter}");
    expect(useStore.getState().activeTab.alpha).toBe("file:projects/web/notes.md");
    expect(useStore.getState().fileTabs.alpha).toHaveLength(1);
  });

  it("marks the row of the active File tab", async () => {
    show([web]);
    await flush();
    const row = within(screen.getByRole("list", { name: "Changes" })).getByRole("button", {
      name: /app\.ts/,
    });
    expect(row).not.toHaveAttribute("aria-current");
    await userEvent.click(row);
    expect(row).toHaveAttribute("aria-current", "true");
  });

  it("doesn't open an untracked folder", async () => {
    show([repo("web", { untracked: ["scratch/"] })]);
    await flush();
    const untracked = within(screen.getByRole("list", { name: "Untracked" }));
    expect(untracked.getByText("scratch/")).toBeInTheDocument();
    expect(untracked.queryByRole("button")).toBeNull();
  });
});
