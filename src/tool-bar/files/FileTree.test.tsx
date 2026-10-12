import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FileTree, type FileTreeProps, pathUpdates } from "./FileTree";

const PATHS = ["README.md", "src/", "src/main.ts", "src/lib/", "src/lib/util.ts"];
const FOLDERS = ["src/", "src/lib/"];

function show(overrides: Partial<FileTreeProps> = {}) {
  const props: FileTreeProps = {
    paths: PATHS,
    folders: FOLDERS,
    statuses: [],
    decorations: new Map(),
    expanded: ["src/"],
    onExpandedChange: vi.fn(),
    onOpen: vi.fn(),
    menu: () => [],
    ...overrides,
  };
  const view = render(<FileTree {...props} />);
  return { ...view, props };
}

async function settle() {
  await act(() => new Promise((resolve) => setTimeout(resolve, 20)));
}

function rows(): HTMLElement[] {
  const host = document.querySelector("file-tree-container");
  return [...(host?.shadowRoot?.querySelectorAll<HTMLElement>("[data-item-path]") ?? [])];
}

function row(path: string): HTMLElement {
  const found = rows().find((r) => r.dataset.itemPath === path);
  if (!found) throw new Error(`no row for ${path}: ${rows().map((r) => r.dataset.itemPath)}`);
  return found;
}

function fire(target: HTMLElement, event: Event) {
  act(() => {
    target.dispatchEvent(event);
  });
}

describe("FileTree", () => {
  it("shows the paths with the wanted folders open", async () => {
    show();
    await settle();
    expect(rows().map((r) => r.dataset.itemPath)).toEqual([
      "src/",
      "src/lib/",
      "src/main.ts",
      "README.md",
    ]);
  });

  it("follows the expanded folders it's given", async () => {
    const { rerender, props } = show();
    await settle();
    rerender(<FileTree {...props} expanded={["src/", "src/lib/"]} />);
    await settle();
    expect(rows().map((r) => r.dataset.itemPath)).toContain("src/lib/util.ts");
    rerender(<FileTree {...props} expanded={[]} />);
    await settle();
    expect(rows().map((r) => r.dataset.itemPath)).toEqual(["src/", "README.md"]);
  });

  it("reports a folder the user collapses", async () => {
    const { props } = show();
    await settle();
    fire(row("src/"), new MouseEvent("click", { bubbles: true, composed: true }));
    await settle();
    expect(props.onExpandedChange).toHaveBeenLastCalledWith([]);
  });

  it("keeps wanted folders it can't show yet", async () => {
    const { props } = show({ expanded: ["src/", "gone/"] });
    await settle();
    fire(row("src/lib/"), new MouseEvent("click", { bubbles: true, composed: true }));
    await settle();
    expect(new Set(vi.mocked(props.onExpandedChange).mock.lastCall?.[0])).toEqual(
      new Set(["gone/", "src/", "src/lib/"]),
    );
  });

  it("opens a file on double-click, not a folder", async () => {
    const { props } = show();
    await settle();
    fire(row("src/"), new MouseEvent("dblclick", { bubbles: true, composed: true }));
    fire(row("src/main.ts"), new MouseEvent("dblclick", { bubbles: true, composed: true }));
    expect(props.onOpen).toHaveBeenCalledTimes(1);
    expect(props.onOpen).toHaveBeenCalledWith("src/main.ts");
  });

  it("opens the focused file on Enter", async () => {
    const { props } = show();
    await settle();
    fire(row("README.md"), new MouseEvent("click", { bubbles: true, composed: true }));
    await settle();
    fire(
      row("README.md"),
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, composed: true }),
    );
    expect(props.onOpen).toHaveBeenCalledWith("README.md");
  });

  it("marks git status and ignored entries", async () => {
    show({
      statuses: [
        { path: "src/main.ts", status: "modified" },
        { path: "README.md", status: "ignored" },
      ],
    });
    await settle();
    expect(row("src/main.ts").dataset.itemGitStatus).toBe("modified");
    expect(row("README.md").dataset.itemGitStatus).toBe("ignored");
  });

  it("decorates folders with their text", async () => {
    const { rerender, props } = show({ decorations: new Map([["src/", "main"]]) });
    await settle();
    expect(row("src/").textContent).toContain("main");
    rerender(<FileTree {...props} decorations={new Map([["src/", "feat/x"]])} />);
    await settle();
    expect(row("src/").textContent).toContain("feat/x");
  });

  it("runs a context menu action for the right-clicked row", async () => {
    const run = vi.fn();
    const menu = vi.fn(() => [{ label: "Copy relative path", run }]);
    show({ menu });
    await settle();
    fire(row("src/main.ts"), new MouseEvent("contextmenu", { bubbles: true, composed: true }));
    await settle();
    await userEvent.click(screen.getByRole("menuitem", { name: "Copy relative path" }));
    expect(menu).toHaveBeenCalledWith("src/main.ts");
    expect(run).toHaveBeenCalled();
  });
});

describe("pathUpdates", () => {
  it("removes folders with their contents, then adds shallowest first", () => {
    expect(pathUpdates(["a/", "a/b.ts", "c.ts"], ["c.ts", "d/", "d/e.ts"])).toEqual([
      { type: "remove", path: "a/", recursive: true },
      { type: "add", path: "d/" },
      { type: "add", path: "d/e.ts" },
    ]);
  });

  it("does nothing when nothing changed", () => {
    expect(pathUpdates(["a.ts"], ["a.ts"])).toEqual([]);
  });
});
