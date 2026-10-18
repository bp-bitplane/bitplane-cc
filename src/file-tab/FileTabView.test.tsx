import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useStore } from "../store/store";
import type { FileTab } from "../tabs/fileTabs";
import { mockBackend, plane, planesModel, seed } from "../test/fixtures";
import { FileTabView } from "./FileTabView";

vi.mock("./DiffView", () => ({
  default: (props: { kind: string; diffStyle?: string; contents?: string }) => (
    <div data-testid="diff-view" data-kind={props.kind} data-style={props.diffStyle}>
      {props.contents}
    </div>
  ),
}));

const tab = (mode: FileTab["mode"] = "worktree"): FileTab => ({
  planeId: "a",
  path: "web/src/foo.ts",
  mode,
  pinned: true,
});

const PATCH = [
  "diff --git a/src/foo.ts b/src/foo.ts",
  "--- a/src/foo.ts",
  "+++ b/src/foo.ts",
  "@@ -3,3 +3,3 @@",
  " one",
  "-two",
  "+deux",
  " three",
  "",
].join("\n");

describe("FileTabView", () => {
  beforeEach(() => seed({ planes: planesModel([plane("a")]) }));

  it("renders a diff, unified by default, and switches layout", async () => {
    const backend = mockBackend({ plane_file_diff: () => ({ kind: "diff", patch: PATCH }) });
    render(<FileTabView tab={tab()} />);
    const view = await screen.findByTestId("diff-view");
    expect(view).toHaveAttribute("data-kind", "patch");
    expect(view).toHaveAttribute("data-style", "unified");
    expect(backend.called("plane_file_diff")[0]?.args).toEqual({
      planeId: "a",
      path: "web/src/foo.ts",
      mode: "worktree",
      force: false,
    });
    await userEvent.click(screen.getByRole("button", { name: "Split" }));
    expect(useStore.getState().diffStyle).toBe("split");
    expect(screen.getByTestId("diff-view")).toHaveAttribute("data-style", "split");
  });

  it("opens VS Code at the first changed line", async () => {
    const backend = mockBackend({ plane_file_diff: () => ({ kind: "diff", patch: PATCH }) });
    render(<FileTabView tab={tab()} />);
    await screen.findByTestId("diff-view");
    await userEvent.click(screen.getByRole("button", { name: "Open in VS Code" }));
    expect(backend.called("plane_open_file")[0]?.args).toEqual({
      planeId: "a",
      path: "web/src/foo.ts",
      line: 4,
    });
  });

  it("shows a rename as old → new", async () => {
    const patch =
      "diff --git a/a.ts b/b.ts\nsimilarity index 100%\nrename from a.ts\nrename to b.ts\n";
    mockBackend({ plane_file_diff: () => ({ kind: "diff", patch }) });
    render(<FileTabView tab={tab("index")} />);
    expect(await screen.findByText("a.ts → b.ts")).toBeInTheDocument();
  });

  it("shows plain contents when there's no diff", async () => {
    mockBackend({
      plane_file_diff: () => ({ kind: "plain" }),
      plane_read_file: () => ({ kind: "text", contents: "hello" }),
    });
    render(<FileTabView tab={tab("file")} />);
    const view = await screen.findByTestId("diff-view");
    expect(view).toHaveAttribute("data-kind", "file");
    expect(view).toHaveTextContent("hello");
    expect(screen.queryByRole("button", { name: "Split" })).toBeNull();
  });

  it("says when the change is gone", async () => {
    mockBackend({ plane_file_diff: () => ({ kind: "none" }) });
    render(<FileTabView tab={tab("index")} />);
    expect(await screen.findByRole("heading", { name: "No changes" })).toBeInTheDocument();
    expect(screen.getByText("web/src/foo.ts has no staged changes anymore.")).toBeInTheDocument();
  });

  it("shows binary files with their sizes", async () => {
    mockBackend({ plane_file_diff: () => ({ kind: "binary", oldSize: 512, newSize: 2048 }) });
    render(<FileTabView tab={tab()} />);
    expect(await screen.findByText("512 B → 2.0 KB")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Open in VS Code" })).toHaveLength(2);
  });

  it("reads a too-large file again when asked to show it anyway", async () => {
    const backend = mockBackend({
      plane_file_diff: ({ force }) =>
        force ? { kind: "diff", patch: PATCH } : { kind: "tooLarge", size: 3 * 1024 * 1024 },
    });
    render(<FileTabView tab={tab()} />);
    expect(await screen.findByText("3.0 MB")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show anyway" }));
    expect(await screen.findByTestId("diff-view")).toBeInTheDocument();
    expect(backend.called("plane_file_diff").map((c) => c.args.force)).toEqual([false, true]);
  });

  it("holds back a diff with too many changed lines", async () => {
    const lines = Array.from({ length: 20_001 }, (_, i) => `+${i}`).join("\n");
    const patch = `diff --git a/x b/x\n@@ -0,0 +1,20001 @@\n${lines}\n`;
    mockBackend({ plane_file_diff: () => ({ kind: "diff", patch }) });
    render(<FileTabView tab={tab()} />);
    expect(await screen.findByText("20,001 changed lines")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show anyway" }));
    expect(await screen.findByTestId("diff-view")).toBeInTheDocument();
  });

  it("shows why a file couldn't be read", async () => {
    mockBackend({
      plane_file_diff: () => {
        throw "web/src no longer exists";
      },
    });
    render(<FileTabView tab={tab()} />);
    expect(await screen.findByText("web/src no longer exists")).toBeInTheDocument();
  });

  it("shows the panel's notice for a gone plane", () => {
    const backend = mockBackend();
    seed({ planes: planesModel([plane("a", "gone")]) });
    render(<FileTabView tab={tab()} />);
    expect(screen.getByRole("heading", { name: "a is gone" })).toBeInTheDocument();
    expect(backend.called("plane_file_diff")).toEqual([]);
  });
});
