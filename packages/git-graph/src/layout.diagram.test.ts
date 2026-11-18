import { diagram, toAscii } from "../test/ascii";
import { commit as c, layout } from "../test/layout";

describe("layout diagrams", () => {
  test("linear history stays on one rail", () => {
    expect(toAscii(layout([c("1", ["2"]), c("2", ["3"]), c("3", [])]))).toBe(diagram`
      *  1
      *  2
      *  3
    `);
  });

  test("a side branch opens a rail and ends in the fork point", () => {
    expect(
      toAscii(layout([c("1", ["2a"]), c("2a", ["3"]), c("2b", ["3"]), c("3", ["4"]), c("4", [])])),
    ).toBe(diagram`
      *  1
      *  2a
      |*  2b
      *┘  3
      *  4
    `);
  });

  test("a merge opens a rail for its second parent", () => {
    expect(
      toAscii(
        layout([c("1", ["2a", "2b"]), c("2a", ["3"]), c("2b", ["3"]), c("3", ["4"]), c("4", [])]),
      ),
    ).toBe(diagram`
      *┐  1
      *|  2a
      |*  2b
      *┘  3
      *  4
    `);
  });

  test("an unrelated branch head between a merge and its parents gets its own rail", () => {
    expect(
      toAscii(
        layout([
          c("1", ["2a", "2b"]),
          c("2c", ["4"]),
          c("2a", ["3"]),
          c("2b", ["3"]),
          c("3", ["4"]),
          c("4", []),
        ]),
      ),
    ).toBe(diagram`
      *┐  1
      ||*  2c
      *||  2a
      |*|  2b
      *┘|  3
      *┘  4
    `);
  });

  test("merging the next commit joins its existing rail", () => {
    expect(
      toAscii(
        layout([
          c("a1", ["a2"]),
          c("b1", ["b2"]),
          c("a2", ["a3", "b2"]),
          c("b2", ["b3"]),
          c("a3", []),
          c("b3", []),
        ]),
      ),
    ).toBe(diagram`
      *  a1
      |*  b1
      *┤  a2
      |*  b2
      *|  a3
      *  b3
    `);
  });

  test("an octopus merge opens a rail per extra parent", () => {
    expect(
      toAscii(
        layout([c("m", ["a", "b", "c"]), c("a", ["r"]), c("b", ["r"]), c("c", ["r"]), c("r", [])]),
      ),
    ).toBe(diagram`
      *┐┐  m
      *||  a
      |*|  b
      ||*  c
      *┘┘  r
    `);
  });

  test("disjoint histories keep separate rails", () => {
    expect(toAscii(layout([c("1", ["2"]), c("x", ["y"]), c("2", []), c("y", [])]))).toBe(diagram`
      *  1
      |*  x
      *|  2
      *  y
    `);
  });

  test("criss-cross merges", () => {
    expect(
      toAscii(
        layout([
          c("m1", ["a", "b"]),
          c("m2", ["b", "a"]),
          c("a", ["r"]),
          c("b", ["r"]),
          c("r", []),
        ]),
      ),
    ).toBe(diagram`
      *┐  m1
      ├|*  m2
      *||  a
      |*┘  b
      *┘  r
    `);
  });

  test("a merge whose second parent comes next but has no rail yet", () => {
    expect(
      toAscii(
        layout([
          c("t", ["m"]),
          c("m", ["a", "f2"]),
          c("f2", ["f1"]),
          c("a", ["b"]),
          c("f1", ["b"]),
          c("b", []),
        ]),
      ),
    ).toBe(diagram`
      *  t
      *┐  m
      |*  f2
      *|  a
      |*  f1
      *┘  b
    `);
  });

  test("the WIP node sits on top of HEAD's rail", () => {
    const index = {
      parents: ["2"],
      tracked: [{ path: "a", mode: "M" as const }],
      untracked: [],
      branch: undefined,
    };
    expect(toAscii(layout([c("1", ["2"]), c("2", ["3"]), c("3", [])], { index }))).toBe(diagram`
      *  WIP
      |*  1
      *┘  2
      *  3
    `);
  });
});
