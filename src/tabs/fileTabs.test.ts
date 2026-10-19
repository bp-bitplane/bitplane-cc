import { type FileTab, fileTabTitle, folderSuffixes } from "./fileTabs";

const tab = (path: string, mode: FileTab["mode"] = "file"): FileTab => ({
  planeId: "a",
  path,
  mode,
  pinned: true,
});

describe("File tab titles", () => {
  it("name the mode like VS Code", () => {
    expect(fileTabTitle(tab("src/foo.ts"))).toBe("foo.ts");
    expect(fileTabTitle(tab("src/foo.ts", "index"))).toBe("foo.ts (Index)");
    expect(fileTabTitle(tab("src/foo.ts", "worktree"))).toBe("foo.ts (Working Tree)");
  });

  it("add the shortest telling folder only when names collide", () => {
    const suffixes = folderSuffixes([
      tab("api/src/index.ts"),
      tab("web/src/index.ts"),
      tab("web/src/index.ts", "index"),
      tab("index.ts"),
      tab("src/main.ts"),
    ]);
    expect(Object.fromEntries(suffixes)).toEqual({
      "file:api/src/index.ts": "api/src",
      "file:web/src/index.ts": "web/src",
      "index:web/src/index.ts": "web/src",
      "file:index.ts": "./",
    });
  });

  it("stop at one folder when it already differs", () => {
    const suffixes = folderSuffixes([tab("a/x/foo.ts"), tab("b/y/foo.ts")]);
    expect([...suffixes.values()]).toEqual(["x", "y"]);
  });
});
