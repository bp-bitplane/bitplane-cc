import { diagram, toAscii } from "../test/ascii";
import { commit, layout } from "../test/layout";
import { createGraph, type GraphCommit, type GraphNode, isWip } from "./createGraph";

test("linear", () => {
  const commits = [commit("1", ["2"]), commit("2", ["3"]), commit("3", ["4"]), commit("4", [])];

  const { nodes } = layout(commits);

  const [n1, n2, n3, n4] = nodes as [GraphNode, GraphNode, GraphNode, GraphNode, GraphNode];

  expect(n1.forks).toMatchObject([]);
  expect(n1.rails).toMatchObject([]);
  expect(n1.position).toBe(0);
  expect(n1.merges).toMatchObject([0]);

  expect(n2.forks).toMatchObject([]);
  expect(n2.rails).toMatchObject([0]);
  expect(n2.position).toBe(0);
  expect(n2.merges).toMatchObject([]);

  expect(n3.forks).toMatchObject([]);
  expect(n3.rails).toMatchObject([0]);
  expect(n3.position).toBe(0);
  expect(n3.merges).toMatchObject([]);

  expect(n4.forks).toMatchObject([0]);
  expect(n4.rails).toMatchObject([0]);
  expect(n4.position).toBe(0);
  expect(n4.merges).toMatchObject([]);
});

test("fork", () => {
  const commits: GraphCommit[] = [
    commit("1", ["2a"]),
    commit("2a", ["3"]),
    commit("2b", ["3"]),
    commit("3", ["4"]),
    commit("4", []),
  ];

  const { nodes } = layout(commits);

  const [n1, n2a, n2b, n3, n4] = nodes as [GraphNode, GraphNode, GraphNode, GraphNode, GraphNode];

  expect(n1.forks).toMatchObject([]);
  expect(n1.rails).toMatchObject([]);
  expect(n1.position).toBe(0);
  expect(n1.merges).toMatchObject([0]);

  expect(n2a.forks).toMatchObject([]);
  expect(n2a.rails).toMatchObject([0]);
  expect(n2a.position).toBe(0);
  expect(n2a.merges).toMatchObject([]);

  expect(n2b.forks).toMatchObject([]);
  expect(n2b.rails).toMatchObject([0]);
  expect(n2b.position).toBe(1);
  expect(n2b.merges).toMatchObject([1]);

  expect(n3.forks).toMatchObject([1]);
  expect(n3.rails).toMatchObject([0, 1]);
  expect(n3.position).toBe(0);
  expect(n3.merges).toMatchObject([]);

  expect(n4.forks).toMatchObject([0]);
  expect(n4.rails).toMatchObject([0]);
  expect(n4.position).toBe(0);
  expect(n4.merges).toMatchObject([]);
});

test("merge", () => {
  const commits: GraphCommit[] = [
    commit("1", ["2a", "2b"]),
    commit("2a", ["3"]),
    commit("2b", ["3"]),
    commit("3", ["4"]),
    commit("4", []),
  ];

  const { nodes } = layout(commits);

  const [n1, n2a, n2b, n3, n4] = nodes as [GraphNode, GraphNode, GraphNode, GraphNode, GraphNode];

  expect(n1.forks).toMatchObject([]);
  expect(n1.rails).toMatchObject([]);
  expect(n1.position).toBe(0);
  expect(n1.merges).toMatchObject([0, 1]);

  expect(n2a.forks).toMatchObject([]);
  expect(n2a.rails).toMatchObject([0, 1]);
  expect(n2a.position).toBe(0);
  expect(n2a.merges).toMatchObject([]);

  expect(n2b.forks).toMatchObject([]);
  expect(n2b.rails).toMatchObject([0, 1]);
  expect(n2b.position).toBe(1);
  expect(n2b.merges).toMatchObject([]);

  expect(n3.forks).toMatchObject([1]);
  expect(n3.rails).toMatchObject([0, 1]);
  expect(n3.position).toBe(0);
  expect(n3.merges).toMatchObject([]);

  expect(n4.forks).toMatchObject([0]);
  expect(n4.rails).toMatchObject([0]);
  expect(n4.position).toBe(0);
  expect(n4.merges).toMatchObject([]);
});

test("merge2", () => {
  const commits: GraphCommit[] = [
    commit("1", ["2a", "2b"]),
    commit("2c", ["4"]),
    commit("2a", ["3"]),
    commit("2b", ["3"]),
    commit("3", ["4"]),
    commit("4", []),
  ];

  const { nodes } = layout(commits);

  const [n1, n2c, n2a, n2b, n3, n4] = nodes as [
    GraphNode,
    GraphNode,
    GraphNode,
    GraphNode,
    GraphNode,
    GraphNode,
  ];

  expect(n1.forks).toMatchObject([]);
  expect(n1.rails).toMatchObject([]);
  expect(n1.position).toBe(0);
  expect(n1.merges).toMatchObject([0, 1]);

  expect(n2c.forks).toMatchObject([]);
  expect(n2c.rails).toMatchObject([0, 1]);
  expect(n2c.position).toBe(2);
  expect(n2c.merges).toMatchObject([2]);

  expect(n2a.forks).toMatchObject([]);
  expect(n2a.rails).toMatchObject([0, 1, 2]);
  expect(n2a.position).toBe(0);
  expect(n2a.merges).toMatchObject([]);

  expect(n2b.forks).toMatchObject([]);
  expect(n2b.rails).toMatchObject([0, 1, 2]);
  expect(n2b.position).toBe(1);
  expect(n2b.merges).toMatchObject([]);

  expect(n3.forks).toMatchObject([1]);
  expect(n3.rails).toMatchObject([0, 1, 2]);
  expect(n3.position).toBe(0);
  expect(n3.merges).toMatchObject([]);

  expect(n4.forks).toMatchObject([2, 0]);
  expect(n4.rails).toMatchObject([0, 2]);
  expect(n4.position).toBe(0);
  expect(n4.merges).toMatchObject([]);
});

test("weird merge, do not create extra rail while merging", () => {
  const commits: GraphCommit[] = [
    commit("a1", ["a2"]),
    commit("b1", ["b2"]),
    commit("a2", ["a3", "b2"]),
    commit("b2", ["b3"]),
    commit("a3", []),
    commit("b3", []),
  ];

  const { nodes } = layout(commits);

  const [a1, b1, a2, b2, a3, b3] = nodes as [
    GraphNode,
    GraphNode,
    GraphNode,
    GraphNode,
    GraphNode,
    GraphNode,
  ];

  expect(a1.forks).toEqual([]);
  expect(a1.rails).toEqual([]);
  expect(a1.position).toBe(0);
  expect(a1.merges).toEqual([0]);

  expect(b1.forks).toEqual([]);
  expect(b1.rails).toEqual([0]);
  expect(b1.position).toBe(1);
  expect(b1.merges).toEqual([1]);

  expect(a2.forks).toEqual([]);
  expect(a2.rails).toEqual([0, 1]);
  expect(a2.position).toBe(0);
  expect(a2.merges).toEqual([1]);

  expect(b2.forks).toEqual([]);
  expect(b2.rails).toEqual([0, 1]);
  expect(b2.position).toBe(1);
  expect(b2.merges).toEqual([]);

  expect(a3.forks).toEqual([0]);
  expect(a3.rails).toEqual([0, 1]);
  expect(a3.position).toBe(0);
  expect(a3.merges).toEqual([]);

  expect(b3.forks).toEqual([1]);
  expect(b3.rails).toEqual([1]);
  expect(b3.position).toBe(1);
  expect(b3.merges).toEqual([]);
});

test("wip", () => {
  const wip = { parents: ["2"] };

  const commits: GraphCommit[] = [
    commit("1", ["2"]),
    commit("2", ["3"]),
    commit("3", ["4"]),
    commit("4", []),
  ];

  const { nodes } = layout(commits, { wip });

  const [i, n1, n2, n3, n4] = nodes as [GraphNode, GraphNode, GraphNode, GraphNode, GraphNode];

  expect(i.forks).toMatchObject([]);
  expect(i.rails).toMatchObject([]);
  expect(i.position).toBe(0);
  expect(i.merges).toMatchObject([0]);

  expect(n1.forks).toMatchObject([]);
  expect(n1.position).toBe(1);
  expect(n1.rails).toMatchObject([0]);
  expect(n1.merges).toMatchObject([1]);

  expect(n2.forks).toMatchObject([1]);
  expect(n2.position).toBe(0);
  expect(n2.rails).toMatchObject([0, 1]);
  expect(n2.merges).toMatchObject([]);

  expect(n3.forks).toMatchObject([]);
  expect(n3.position).toBe(0);
  expect(n3.rails).toMatchObject([0]);
  expect(n3.merges).toMatchObject([]);

  expect(n4.forks).toMatchObject([0]);
  expect(n4.position).toBe(0);
  expect(n4.rails).toMatchObject([0]);
  expect(n4.merges).toMatchObject([]);
});

test("single", () => {
  const commits: GraphCommit[] = [commit("1", [])];

  const { nodes } = layout(commits);

  const [n1] = nodes as [GraphNode];

  expect(n1.forks).toMatchObject([]);
  expect(n1.position).toBe(0);
  expect(n1.rails).toMatchObject([]);
  expect(n1.merges).toMatchObject([]);
});

describe("hidden", () => {
  test("side branch", () => {
    const commits: GraphCommit[] = [
      commit("1", ["2a"]),
      commit("2a", ["3"]),
      commit("2b", ["3"]),
      commit("3", ["4"]),
      commit("4", []),
    ];

    const { nodes } = layout(commits, { visibleTips: new Set(["2b"]) });

    const hashes = nodes.map((n) => (n.commit as GraphCommit).hash);

    expect(hashes).toEqual(["2b", "3", "4"]);
  });

  test("side branch - hidden first", () => {
    const commits: GraphCommit[] = [
      commit("1", ["2a"]),
      commit("2a", ["3"]),
      commit("2b", ["3"]),
      commit("3", ["4"]),
      commit("4", []),
    ];

    const { nodes } = layout(commits, { visibleTips: new Set(["3"]) });

    const hashes = nodes.map((n) => (n.commit as GraphCommit).hash);

    expect(hashes).toEqual(["3", "4"]);
  });

  test("after hidden", () => {
    const commits: GraphCommit[] = [
      commit("1", ["2"]),
      commit("2", ["3"]),
      commit("3", ["4"]),
      commit("4", []),
    ];

    const { nodes } = layout(commits, { visibleTips: new Set(["2"]) });

    const hashes = nodes.map((n) => (n.commit as GraphCommit).hash);

    expect(hashes).toEqual(["2", "3", "4"]);
  });

  test("merge", () => {
    const commits: GraphCommit[] = [
      commit("1", ["2a", "2b"]),
      commit("2a", ["3"]),
      commit("2b", ["3"]),
      commit("3", ["4"]),
      commit("4", []),
    ];

    const { nodes } = layout(commits, { visibleTips: new Set(["1"]) });

    const hashes = nodes.map((n) => (n.commit as GraphCommit).hash);

    expect(hashes).toEqual(["1", "2a", "2b", "3", "4"]);
  });
});

describe("options", () => {
  const withStash = [commit("s", ["2", "i"]), commit("1", ["2"]), commit("2", [])];

  test("lays out every commit by default", () => {
    const commits = [
      commit("1", ["2a"]),
      commit("2b", ["3"]),
      commit("2a", ["3"]),
      commit("3", []),
    ];

    expect(layout(commits).nodes).toHaveLength(4);
  });

  test("an empty visibleTips hides every commit", () => {
    expect(layout([commit("1", ["2"]), commit("2", [])], { visibleTips: new Set() }).nodes).toEqual(
      [],
    );
  });

  test("ignoreMergeParentsOf draws a stash on its base without a rail to its index commit", () => {
    const graph = layout(withStash, { ignoreMergeParentsOf: new Set(["s"]) });

    expect(toAscii(graph)).toBe(diagram`
      *  s
      |*  1
      *┘  2
    `);
    expect(graph.rails.rails).toEqual([]);
  });

  test("without ignoreMergeParentsOf a stash keeps a rail open to its index commit", () => {
    expect(layout(withStash).rails.rails.map((r) => r.next)).toEqual(["i"]);
  });
});

describe("builder", () => {
  test("holds the last appended commit back until the next append or end", () => {
    const graph = createGraph();

    graph.append([commit("1", ["2"]), commit("2", ["3"])]);
    expect(graph.nodes.map(hashOf)).toEqual(["1"]);

    graph.append([commit("3", [])]);
    expect(graph.nodes.map(hashOf)).toEqual(["1", "2"]);

    graph.end();
    expect(graph.nodes.map(hashOf)).toEqual(["1", "2", "3"]);
  });
});

const hashOf = (n: GraphNode) => (isWip(n.commit) ? "WIP" : n.commit.hash);
