import fc from "fast-check";
import { toAscii } from "../test/ascii";
import { commit as c, layout, layoutInChunks } from "../test/layout";
import type { GraphNode } from "./createGraphNodes";
import type { GitCommit } from "./git";

describe("layout invariants over random histories", () => {
  test("every commit becomes exactly one node, in log order", () => {
    fc.assert(
      fc.property(history(), (commits) => {
        expect(layout(commits).nodes.map(hashOf)).toEqual(commits.map((c) => c.hash));
      }),
    );
  });

  test("each node's rails are unique, forks end rails that exist, merges join at most one", () => {
    fc.assert(
      fc.property(history(), (commits) => {
        for (const n of layout(commits).nodes) {
          expect(new Set(n.rails).size).toBe(n.rails.length);
          expect(n.rails).toEqual(expect.arrayContaining(n.forks));
          expect(n.merges.filter((m) => n.rails.includes(m)).length).toBeLessThanOrEqual(1);
        }
      }),
    );
  });

  test("a rail only appears below the node that opened or carried it", () => {
    fc.assert(
      fc.property(history(), (commits) => {
        const { nodes } = layout(commits);
        for (const [i, n] of nodes.entries()) {
          const below = nodes[i + 1]?.rails ?? [];
          const known = new Set([...n.rails, ...n.merges, n.position]);
          expect(below.filter((r) => !known.has(r))).toEqual([]);
        }
      }),
    );
  });

  test("every parent edge arrives at the parent's node", () => {
    fc.assert(
      fc.property(history(), (commits) => {
        const nodes = new Map(layout(commits).nodes.map((n) => [hashOf(n), n]));
        for (const n of nodes.values()) {
          const [first, ...rest] = n.commit.parents;
          if (first === undefined) continue;
          expect(arrivals(nodes.get(first)!)).toContain(n.position);
          for (const p of rest) {
            const reached = n.merges.filter((m) => arrivals(nodes.get(p)!).includes(m));
            expect(reached, `${hashOf(n)} -> ${p}`).not.toEqual([]);
          }
        }
      }),
    );
  });

  test("no rail is left open once every parent is laid out", () => {
    fc.assert(
      fc.property(history(), (commits) => {
        expect(layout(commits).rails.rails).toEqual([]);
      }),
    );
  });

  test("feeding the log in chunks lays it out the same as all at once", () => {
    fc.assert(
      fc.property(
        history().chain((commits) =>
          fc.tuple(fc.constant(commits), fc.uniqueArray(fc.nat(commits.length))),
        ),
        ([commits, cuts]) => {
          fc.pre(cuts.every((cut) => !mergesNext(commits[cut - 1], commits[cut])));
          const bounds = [0, ...cuts.sort((a, b) => a - b), commits.length];
          const chunks = bounds.slice(1).map((end, i) => commits.slice(bounds[i], end));
          expect(shape(layoutInChunks(chunks).nodes)).toEqual(shape(layout(commits).nodes));
        },
      ),
    );
  });

  test.fails("a chunk boundary right after a merge of the next commit keeps the join", () => {
    const commits = [
      c("a1", ["a2"]),
      c("b1", ["b2"]),
      c("a2", ["a3", "b2"]),
      c("b2", []),
      c("a3", []),
    ];
    expect(toAscii(layoutInChunks([commits.slice(0, 3), commits.slice(3)]))).toBe(
      toAscii(layout(commits)),
    );
  });
});

/** Commits newest first, each parent older than its child, as `git log --date-order` emits. */
const history = (): fc.Arbitrary<GitCommit[]> =>
  fc
    .integer({ min: 1, max: 30 })
    .chain((size) =>
      fc.tuple(
        ...Array.from({ length: size }, (_, i) =>
          i + 1 < size
            ? fc.uniqueArray(fc.integer({ min: i + 1, max: size - 1 }), { maxLength: 3 })
            : fc.constant([]),
        ),
      ),
    )
    .map((parents) =>
      parents.map((ps, i) =>
        c(
          `c${i}`,
          ps.map((p) => `c${p}`),
        ),
      ),
    );

const mergesNext = (merge: GitCommit | undefined, next: GitCommit | undefined) =>
  !!merge && !!next && merge.parents.slice(1).includes(next.hash);

const hashOf = (n: GraphNode) => ("hash" in n.commit ? n.commit.hash : "WIP");

const arrivals = (n: GraphNode) => [n.position, ...n.forks];

const shape = (nodes: GraphNode[]) =>
  nodes.map((n) => ({
    hash: hashOf(n),
    position: n.position,
    rails: [...n.rails],
    forks: [...n.forks],
    merges: [...n.merges],
  }));
