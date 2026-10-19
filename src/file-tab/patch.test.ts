import { changedLines, firstChangedLine, renameOf } from "./patch";

const PATCH = [
  "diff --git a/a.ts b/a.ts",
  "--- a/a.ts",
  "+++ b/a.ts",
  "@@ -10,4 +10,5 @@ function f() {",
  " a",
  " b",
  "+c",
  "-d",
  "+e",
  "",
].join("\n");

describe("patch", () => {
  it("counts added and removed lines, not headers", () => {
    expect(changedLines(PATCH)).toBe(3);
  });

  it("finds the new-side line of the first change", () => {
    expect(firstChangedLine(PATCH)).toBe(12);
    expect(firstChangedLine("diff --git a/x b/x\nrename from x\nrename to y\n")).toBe(1);
    expect(firstChangedLine("@@ -1,2 +0,0 @@\n-a\n-b\n")).toBe(1);
  });

  it("reads renames", () => {
    expect(renameOf("rename from a/b.ts\nrename to a/c.ts\n")).toEqual({
      from: "a/b.ts",
      to: "a/c.ts",
    });
    expect(renameOf(PATCH)).toBeNull();
  });
});
