import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { FileEntry } from "../../ipc";
import { applyFilesChanged, useStore } from "../../store/store";
import { mockBackend, plane, planesModel, repo, seed } from "../../test/fixtures";
import { run } from "../../useShortcuts";
import { ToolPanel } from "../ToolPanel";
import { ERROR_ROW } from "./filesTree";

const file = (name: string, ignored = false): FileEntry => ({ name, kind: "file", ignored });
const dir = (name: string, ignored = false): FileEntry => ({ name, kind: "directory", ignored });

const web = repo("web", {
  head: { kind: "branch", name: "feat/x" },
  unstaged: [{ path: "a.ts", kind: "modified", from: null }],
});

let disk: Record<string, FileEntry[]>;

function show(edit: (disk: Record<string, FileEntry[]>) => void = () => {}) {
  disk = {
    "": [dir("projects"), file("plane.toml")],
    projects: [dir("web")],
    "projects/web": [dir(".git", true), dir("dist", true), file("a.ts")],
    "projects/web/dist": [file("out.js")],
  };
  edit(disk);
  const backend = mockBackend({
    plane_source_control: () => [web],
    plane_list_dir: ({ path }) => {
      const entries = disk[path as string];
      if (!entries) throw new Error(`${path} no longer exists`);
      return entries;
    },
  });
  seed({
    planes: planesModel([plane("alpha")]),
    selectedPlaneId: "alpha",
    toolPanelOpen: true,
    activeTool: "files",
  });
  const view = render(<ToolPanel />);
  return { ...view, backend };
}

async function settle() {
  await act(() => new Promise((resolve) => setTimeout(resolve, 30)));
}

function rows(): HTMLElement[] {
  const host = document.querySelector("file-tree-container");
  return [...(host?.shadowRoot?.querySelectorAll<HTMLElement>("[data-item-path]") ?? [])];
}

function paths(): (string | undefined)[] {
  return rows().map((r) => r.dataset.itemPath);
}

function row(path: string): HTMLElement {
  const found = rows().find((r) => r.dataset.itemPath === path);
  if (!found) throw new Error(`no row for ${path}: ${paths()}`);
  return found;
}

function listed(backend: ReturnType<typeof mockBackend>): unknown[] {
  return backend.called("plane_list_dir").map((c) => c.args.path);
}

describe("Files", () => {
  it("opens on the plane root with the Members' folders expanded", async () => {
    const { backend } = show();
    await settle();
    expect(listed(backend)).toEqual(["", "projects", "projects/web"]);
    expect(paths()).toContain("projects/web/a.ts");
    expect(useStore.getState().filesExpanded.alpha).toEqual(["projects/", "projects/web/"]);
  });

  it("shows each Member's branch, git status and dimmed ignored entries", async () => {
    show();
    await settle();
    expect(rows().some((r) => r.textContent?.includes("feat/x"))).toBe(true);
    expect(row("projects/web/a.ts").dataset.itemGitStatus).toBe("modified");
    expect(row("projects/web/.git/").dataset.itemGitStatus).toBe("ignored");
    expect(row("projects/web/dist/").dataset.itemGitStatus).toBe("ignored");
  });

  it("lists a folder when it's expanded and watches the open folders", async () => {
    const { backend } = show();
    await settle();
    act(() => {
      row("projects/web/dist/").dispatchEvent(
        new MouseEvent("click", { bubbles: true, composed: true }),
      );
    });
    await settle();
    expect(listed(backend)).toContain("projects/web/dist");
    expect(paths()).toContain("projects/web/dist/out.js");
    expect(backend.called("plane_files_watch").at(-1)?.args).toEqual({
      planeId: "alpha",
      dirs: ["", "projects", "projects/web", "projects/web/dist"],
    });
  });

  it("lists a folder again when the watcher reports it", async () => {
    const { backend } = show();
    await settle();
    disk["projects/web"] = [...(disk["projects/web"] ?? []), file("b.ts")];
    act(() => applyFilesChanged({ planeId: "alpha", dirs: ["projects/web"] }));
    act(() => applyFilesChanged({ planeId: "beta", dirs: ["projects"] }));
    await settle();
    expect(listed(backend).slice(3)).toEqual(["projects/web"]);
    expect(paths()).toContain("projects/web/b.ts");
  });

  it("drops a folder that disappeared", async () => {
    show();
    await settle();
    disk.projects = [];
    delete disk["projects/web"];
    act(() => applyFilesChanged({ planeId: "alpha", dirs: ["projects", "projects/web"] }));
    await settle();
    expect(paths()).toEqual(["projects/", "plane.toml"]);
  });

  it("lists every folder and reads git status again on Refresh and ⌘⇧R", async () => {
    const { backend } = show();
    await settle();
    await userEvent.click(screen.getByRole("button", { name: /^Refresh/ }));
    await settle();
    expect(listed(backend).slice(3).sort()).toEqual(["", "projects", "projects/web"]);
    expect(backend.called("plane_source_control")).toHaveLength(2);
    act(() => run({ action: "refresh" }));
    await settle();
    expect(listed(backend)).toHaveLength(9);
  });

  it("collapses every folder", async () => {
    show();
    await settle();
    await userEvent.click(screen.getByRole("button", { name: "Collapse all" }));
    await settle();
    expect(paths()).toEqual(["projects/web/", "plane.toml"]);
    expect(useStore.getState().filesExpanded.alpha).toEqual([]);
  });

  it("keeps the expanded folders when it's shown again", async () => {
    const { unmount } = show();
    await settle();
    await userEvent.click(screen.getByRole("button", { name: "Collapse all" }));
    unmount();
    render(<ToolPanel />);
    await settle();
    expect(paths()).toEqual(["projects/", "plane.toml"]);
  });

  it("opens a file in VS Code on double-click", async () => {
    const { backend } = show();
    await settle();
    act(() => {
      row("projects/web/a.ts").dispatchEvent(
        new MouseEvent("dblclick", { bubbles: true, composed: true }),
      );
    });
    await settle();
    expect(backend.called("plane_open_file")).toEqual([
      { cmd: "plane_open_file", args: { planeId: "alpha", path: "projects/web/a.ts", line: null } },
    ]);
  });

  it("copies a row's absolute and relative path", async () => {
    show();
    await settle();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    for (const label of ["Copy path", "Copy relative path"]) {
      act(() => {
        row("projects/web/a.ts").dispatchEvent(
          new MouseEvent("contextmenu", { bubbles: true, composed: true }),
        );
      });
      await settle();
      await userEvent.click(screen.getByRole("menuitem", { name: label }));
      await settle();
    }
    expect(writeText.mock.calls).toEqual([
      ["/planes/alpha/projects/web/a.ts"],
      ["projects/web/a.ts"],
    ]);
  });

  it("stops watching when it closes", async () => {
    const { backend, unmount } = show();
    await settle();
    unmount();
    expect(backend.called("plane_files_watch").at(-1)?.args).toEqual({ planeId: null, dirs: [] });
    expect(backend.called("plane_source_control_watch").at(-1)?.args).toEqual({ planeId: null });
  });

  it("explains a plane root it can't list", async () => {
    show((disk) => {
      delete disk[""];
    });
    await settle();
    expect(screen.getByText("Couldn't list files")).toBeInTheDocument();
  });

  it("shows why a folder couldn't be listed and tries again on Refresh", async () => {
    const { backend } = show((disk) => {
      disk.projects = [dir("web"), dir("notes")];
    });
    await settle();
    act(() => {
      row("projects/notes/").dispatchEvent(
        new MouseEvent("click", { bubbles: true, composed: true }),
      );
    });
    await settle();
    const error = row(`projects/notes/${ERROR_ROW}`);
    expect(error.textContent).toContain("projects/notes no longer exists");
    act(() => {
      error.dispatchEvent(new MouseEvent("dblclick", { bubbles: true, composed: true }));
    });
    expect(backend.called("plane_open_file")).toEqual([]);

    disk["projects/notes"] = [file("a.md")];
    await userEvent.click(screen.getByRole("button", { name: /^Refresh/ }));
    await settle();
    expect(paths()).toContain("projects/notes/a.md");
    expect(paths()).not.toContain(`projects/notes/${ERROR_ROW}`);
  });

  it("toggles with ⌘⇧E", () => {
    seed({ toolPanelOpen: false });
    act(() => run({ action: "files" }));
    expect(useStore.getState()).toMatchObject({ toolPanelOpen: true, activeTool: "files" });
    act(() => run({ action: "files" }));
    expect(useStore.getState().toolPanelOpen).toBe(false);
  });
});
