import type { FileEntry, Repo } from "../../ipc";
import { repo } from "../../test/fixtures";
import { ERROR_ROW, fileStatuses, initialExpanded, repoBranches, treeData } from "./filesTree";

const file = (name: string, ignored = false): FileEntry => ({ name, kind: "file", ignored });
const dir = (name: string, ignored = false): FileEntry => ({ name, kind: "directory", ignored });

const web = repo("web", {
  head: { kind: "branch", name: "feat/x" },
  staged: [{ path: "src/new.ts", kind: "added", from: null }],
  unstaged: [
    { path: "src/app.ts", kind: "modified", from: null },
    { path: "src/new.ts", kind: "modified", from: null },
    { path: "old.ts", kind: "deleted", from: null },
  ],
  untracked: ["notes.md", "tmp/"],
});
const api = repo("api", { head: { kind: "detached", sha: "0123456" } });
const missing: Repo = {
  name: "gone",
  path: "/planes/alpha/projects/gone",
  state: { kind: "missing" },
};

describe("treeData", () => {
  it("walks listed folders from the root, marking folders and ignored entries", () => {
    const listings = new Map([
      ["", [dir("projects"), file("plane.toml")]],
      ["projects", [dir("web")]],
      ["projects/web", [dir(".git", true), dir("node_modules", true), file("a.ts")]],
      ["unreachable", [file("x")]],
    ]);
    expect(treeData(listings)).toEqual({
      paths: [
        "projects/",
        "projects/web/",
        "projects/web/.git/",
        "projects/web/node_modules/",
        "projects/web/a.ts",
        "plane.toml",
      ],
      folders: ["projects/", "projects/web/", "projects/web/.git/", "projects/web/node_modules/"],
      ignored: [
        { path: "projects/web/.git/", status: "ignored" },
        { path: "projects/web/node_modules/", status: "ignored" },
      ],
      errors: new Map(),
    });
  });

  it("stands a row in for a folder that couldn't be listed", () => {
    const listings = new Map([["", [dir("notes"), dir("gone")]]]);
    const errors = new Map([["notes", "couldn't read notes: denied"]]);
    const data = treeData(listings, errors);
    expect(data.paths).toEqual(["notes/", `notes/${ERROR_ROW}`, "gone/"]);
    expect(data.errors).toEqual(new Map([[`notes/${ERROR_ROW}`, "couldn't read notes: denied"]]));
  });
});

describe("fileStatuses", () => {
  it("prefixes each repo's changes with its folder, unstaged over staged", () => {
    expect(fileStatuses([web, api, missing], "/planes/alpha")).toEqual([
      { path: "projects/web/src/new.ts", status: "modified" },
      { path: "projects/web/src/app.ts", status: "modified" },
      { path: "projects/web/old.ts", status: "deleted" },
      { path: "projects/web/notes.md", status: "untracked" },
      { path: "projects/web/tmp/", status: "untracked" },
    ]);
  });

  it("lets a git change win over ignored", () => {
    const ignored = [
      { path: "projects/web/notes.md", status: "ignored" as const },
      { path: "projects/web/dist/", status: "ignored" as const },
    ];
    const statuses = fileStatuses([web], "/planes/alpha", ignored);
    expect(statuses).toContainEqual({ path: "projects/web/notes.md", status: "untracked" });
    expect(statuses).toContainEqual({ path: "projects/web/dist/", status: "ignored" });
  });

  it("reads a repo at the plane root without a prefix", () => {
    const root: Repo = { ...web, path: "/planes/alpha" };
    expect(fileStatuses([root], "/planes/alpha/")[0]).toEqual({
      path: "src/new.ts",
      status: "modified",
    });
  });
});

describe("repoBranches", () => {
  it("names each ready repo's folder by its branch or short SHA", () => {
    expect(repoBranches([web, api, missing], "/planes/alpha")).toEqual(
      new Map([
        ["projects/web/", "feat/x"],
        ["projects/api/", "0123456"],
      ]),
    );
  });
});

describe("initialExpanded", () => {
  it("opens the folders leading to each repo but not the repo's own folder", () => {
    expect(initialExpanded([web, api, missing], "/planes/alpha")).toEqual(["projects/"]);
  });

  it("opens nothing for a repo at the plane root", () => {
    expect(initialExpanded([{ ...web, path: "/planes/alpha" }], "/planes/alpha")).toEqual([]);
  });
});
