import { type FileTab, fileTabTitle, folderSuffixes, resolvePlaneLink } from "./fileTabs";

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

describe("Plane links", () => {
  it.each([
    ["README.md", "docs/guide.md", "../README.md"],
    ["docs/setup.md", "docs/guide.md", "setup.md"],
    ["docs/setup.md", "docs/guide.md", "./setup.md"],
    ["docs/api/x.md", "docs/guide.md", "./api/../api/./x.md"],
    ["src/main.ts", "README.md", "src/main.ts"],
  ])("resolve %s from %s's folder, collapsing . and ..", (expected, from, href) => {
    expect(resolvePlaneLink(from, href)).toBe(expected);
  });

  it("drop the #fragment and ?query", () => {
    expect(resolvePlaneLink("docs/guide.md", "setup.md#install")).toBe("docs/setup.md");
    expect(resolvePlaneLink("docs/guide.md", "setup.md?plain=1#L3")).toBe("docs/setup.md");
  });

  it("percent-decode the path", () => {
    expect(resolvePlaneLink("docs/guide.md", "my%20notes.md")).toBe("docs/my notes.md");
    expect(resolvePlaneLink("docs/guide.md", "%C3%BAvod.md")).toBe("docs/úvod.md");
  });

  it("treat /x as plane-root-relative, like a repo-root link on GitHub", () => {
    expect(resolvePlaneLink("docs/guide.md", "/README.md")).toBe("README.md");
    expect(resolvePlaneLink("docs/guide.md", "/src/../LICENSE")).toBe("LICENSE");
  });

  it.each([
    ["above the plane root", "docs/guide.md", "../../README.md"],
    ["above the plane root from it", "README.md", "../x.md"],
    ["through the root and back", "README.md", "/../README.md"],
    ["a folder with a trailing /", "docs/guide.md", "api/"],
    ["a folder as . or ..", "docs/api/x.md", ".."],
    ["the plane root", "docs/guide.md", ".."],
    ["a pure #anchor", "docs/guide.md", "#install"],
    ["an https link", "docs/guide.md", "https://example.com/a.md"],
    ["a mailto link", "docs/guide.md", "mailto:a@b.c"],
    ["any other scheme", "docs/guide.md", "vscode:extension/x"],
    ["a protocol-relative link", "docs/guide.md", "//example.com/a.md"],
    ["a malformed escape", "docs/guide.md", "100%.md"],
  ])("are null for %s", (_, from, href) => {
    expect(resolvePlaneLink(from, href)).toBeNull();
  });
});
