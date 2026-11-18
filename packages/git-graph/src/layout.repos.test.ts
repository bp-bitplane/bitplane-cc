import { toAscii } from "../test/ascii";
import { loadRepo, REPOS } from "../test/fixtures/repos";
import { layout } from "../test/layout";

describe.each(REPOS)("%s history", (name) => {
  const commits = loadRepo(name);
  const graph = layout(commits);

  test("lays out every commit and closes every rail", () => {
    expect(graph.nodes).toHaveLength(commits.length);
    expect(graph.rails.rails).toEqual([]);
  });

  test("matches the recorded diagram", async () => {
    await expect(toAscii(graph)).toMatchFileSnapshot(`../test/fixtures/repos/${name}.txt`);
  });
});
