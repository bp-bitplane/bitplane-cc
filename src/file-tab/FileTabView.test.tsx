import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { fileTabsOf } from "../store/selectors";
import { applyFileTabsChanged, refresh, useStore } from "../store/store";
import type { FileTab } from "../tabs/fileTabs";
import { mockBackend, plane, planesModel, seed } from "../test/fixtures";
import { FileTabView } from "./FileTabView";

vi.mock("./DiffView", () => ({
  default: (props: { kind: string; diffStyle?: string; contents?: string }) => (
    <div data-testid="diff-view" data-kind={props.kind} data-style={props.diffStyle}>
      {props.contents}
    </div>
  ),
  highlightCode: async (code: string, lang: string) =>
    lang === "ts" ? [[{ content: code, color: "#ff0000" }]] : null,
}));

const tab = (mode: FileTab["mode"] = "worktree", path = "web/src/foo.ts"): FileTab => ({
  planeId: "a",
  path,
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

  it("re-reads when its plane's File tabs are pushed", async () => {
    let diff: object = { kind: "diff", patch: PATCH };
    const backend = mockBackend({
      plane_file_diff: () => diff,
      plane_read_file: () => ({ kind: "text", contents: "committed" }),
    });
    render(<FileTabView tab={tab("file")} />);
    await screen.findByTestId("diff-view");
    diff = { kind: "plain" };
    await act(async () => applyFileTabsChanged({ planeId: "other" }));
    expect(backend.called("plane_file_diff")).toHaveLength(1);
    await act(async () => applyFileTabsChanged({ planeId: "a" }));
    expect(await screen.findByText("committed")).toBeInTheDocument();
  });

  it("re-reads on ⌘⇧R", async () => {
    const backend = mockBackend({ plane_file_diff: () => ({ kind: "none" }) });
    useStore.setState({ selectedPlaneId: "a" });
    render(<FileTabView tab={tab("index")} />);
    await screen.findByRole("heading", { name: "No changes" });
    await act(() => refresh());
    expect(backend.called("plane_file_diff")).toHaveLength(2);
  });

  describe("Markdown preview", () => {
    const README = [
      "# Hello",
      "",
      "| a | b |",
      "| - | - |",
      "| 1 | 2 |",
      "",
      "<b>bold</b>",
      "",
      "[docs](https://example.com)",
      "",
      "```ts",
      "const x = 1;",
      "```",
      "",
    ].join("\n");

    const plainFile = (contents = README) =>
      mockBackend({
        plane_file_diff: () => ({ kind: "plain" }),
        plane_read_file: () => ({ kind: "text", contents }),
      });

    it.each(["README.md", "docs/Notes.MARKDOWN"])("opens %s on Preview", async (path) => {
      plainFile();
      renderOpen(tab("file", path));
      await screen.findByRole("heading", { name: "Hello" });
      expect(screen.getByRole("button", { name: "Preview" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute(
        "aria-pressed",
        "false",
      );
    });

    it("renders GFM and switches to Source and back", async () => {
      plainFile();
      renderOpen(tab("file", "README.md"));
      expect(await screen.findByRole("heading", { name: "Hello" })).toBeInTheDocument();
      expect(screen.getByRole("table")).toBeInTheDocument();
      expect(screen.queryByTestId("diff-view")).toBeNull();
      await userEvent.click(screen.getByRole("button", { name: "Source" }));
      expect(screen.getByTestId("diff-view")).toHaveTextContent("# Hello");
      await userEvent.click(screen.getByRole("button", { name: "Preview" }));
      expect(await screen.findByRole("heading", { name: "Hello" })).toBeInTheDocument();
    });

    it("keeps raw HTML escaped", async () => {
      plainFile();
      const { container } = renderOpen(tab("file", "README.md"));
      await screen.findByRole("heading", { name: "Hello" });
      expect(container.querySelector("b")).toBeNull();
      expect(screen.getByText("<b>bold</b>")).toBeInTheDocument();
    });

    it("doesn't navigate on a link click", async () => {
      plainFile();
      renderOpen(tab("file", "README.md"));
      const link = await screen.findByRole("link", { name: "docs" });
      expect(link).toHaveAttribute("href", "https://example.com");
      expect(fireEvent.click(link)).toBe(false);
    });

    it("highlights a fenced code block once its grammar loads", async () => {
      plainFile();
      renderOpen(tab("file", "README.md"));
      const token = await screen.findByText("const x = 1;", { selector: "span" });
      expect(token).toHaveStyle({ color: "#ff0000" });
    });

    it("keeps the preview when the file changes on disk", async () => {
      let contents = README;
      mockBackend({
        plane_file_diff: () => ({ kind: "plain" }),
        plane_read_file: () => ({ kind: "text", contents }),
      });
      renderOpen(tab("file", "README.md"));
      await screen.findByRole("heading", { name: "Hello" });
      contents = "# Changed";
      await act(async () => applyFileTabsChanged({ planeId: "a" }));
      expect(await screen.findByRole("heading", { name: "Changed" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Preview" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    it("isn't offered on a Unified diff", async () => {
      mockBackend({ plane_file_diff: () => ({ kind: "diff", patch: PATCH }) });
      renderOpen(tab("worktree", "README.md"));
      await screen.findByTestId("diff-view");
      expect(screen.getByRole("button", { name: "Split" })).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
    });

    it.each([
      ["worktree", "Working tree"],
      ["index", "Staged"],
    ] as const)("previews both sides of a Split %s diff", async (mode, label) => {
      mockBackend({
        plane_file_diff: () => ({ kind: "diff", patch: PATCH }),
        plane_file_contents: () => ({ old: "# Old", new: "# New" }),
      });
      useStore.setState({ diffStyle: "split" });
      renderOpen(tab(mode, "README.md"));
      await screen.findByTestId("diff-view");
      expect(screen.getByRole("button", { name: "Source" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      await userEvent.click(screen.getByRole("button", { name: "Preview" }));
      expect(await screen.findByRole("heading", { name: "Old" })).toBeInTheDocument();
      expect(screen.getByRole("heading", { name: "New" })).toBeInTheDocument();
      expect(screen.getByText("Before")).toBeInTheDocument();
      expect(screen.getByText(label)).toBeInTheDocument();
      expect(screen.queryByTestId("diff-view")).toBeNull();

      await userEvent.click(screen.getByRole("button", { name: "Unified" }));
      expect(screen.getByTestId("diff-view")).toHaveAttribute("data-style", "unified");
      expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
      await userEvent.click(screen.getByRole("button", { name: "Split" }));
      expect(await screen.findByRole("heading", { name: "New" })).toBeInTheDocument();
    });

    it("shows why a Split diff's sides couldn't be read", async () => {
      mockBackend({
        plane_file_diff: () => ({ kind: "diff", patch: PATCH }),
        plane_file_contents: () => {
          throw new Error("gone");
        },
      });
      useStore.setState({ diffStyle: "split" });
      renderOpen(tab("worktree", "README.md"));
      await screen.findByTestId("diff-view");
      await userEvent.click(screen.getByRole("button", { name: "Preview" }));
      expect(await screen.findByText(/gone/)).toBeInTheDocument();
    });

    it("isn't offered for other files", async () => {
      plainFile();
      renderOpen(tab("file", "notes.txt"));
      await screen.findByTestId("diff-view");
      expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
    });

    it.each([
      ["binary", { kind: "binary", size: 512 }, "512 B"],
      ["too large", { kind: "tooLarge", size: 3 * 1024 * 1024 }, "3.0 MB"],
    ])("isn't offered for a %s file", async (_, text, size) => {
      mockBackend({
        plane_file_diff: () => ({ kind: "plain" }),
        plane_read_file: () => text,
      });
      renderOpen(tab("file", "README.md"));
      expect(await screen.findByText(size)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Preview" })).toBeNull();
    });
  });
});

/** Renders `open` from the store, as `MainArea` does, so the tab's own state round-trips. */
function renderOpen(open: FileTab) {
  useStore.setState({ fileTabs: { a: [open] } });
  return render(<Open />);
}

function Open() {
  const open = useStore((s) => fileTabsOf(s, "a")[0]);
  return open ? <FileTabView tab={open} /> : null;
}
