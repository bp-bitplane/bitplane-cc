import { idFromBranch, planeIdProblem } from "./planeId";

describe("planeIdProblem", () => {
  it.each(["a", "0", "feat-x", "v1.2_rc", "a".repeat(64)])("accepts %s", (id) => {
    expect(planeIdProblem(id, [])).toBeNull();
  });

  it.each([
    ["", "Enter an id"],
    ["-a", "Use lowercase"],
    ["Feat", "Use lowercase"],
    ["a/b", "Use lowercase"],
    ["a".repeat(65), "At most 64"],
    ["bp-x", "reserved"],
  ])("rejects %j", (id, problem) => {
    expect(planeIdProblem(id, [])).toContain(problem);
  });

  it("rejects an existing id", () => {
    expect(planeIdProblem("taken", ["other", "taken"])).toBe("taken already exists");
  });
});

describe("idFromBranch", () => {
  it.each([
    ["feat/Login", "feat-login"],
    ["bp-thing", "thing"],
    ["alfonz/new/feature", "alfonz-new-feature"],
    ["  main ", "main"],
  ])("%s → %s", (branch, id) => {
    expect(idFromBranch(branch)).toBe(id);
  });
});
