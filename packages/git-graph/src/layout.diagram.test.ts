import { diagram, toAscii } from "../test/ascii";
import { SHAPES, WIP } from "../test/fixtures/shapes";
import { layout } from "../test/layout";

describe("layout diagrams", () => {
  test("linear history stays on one rail", () => {
    expect(toAscii(layout(SHAPES.linear))).toBe(diagram`
      *  1
      *  2
      *  3
    `);
  });

  test("a side branch opens a rail and ends in the fork point", () => {
    expect(toAscii(layout(SHAPES.sideBranch))).toBe(diagram`
      *  1
      *  2a
      |*  2b
      *┘  3
      *  4
    `);
  });

  test("a merge opens a rail for its second parent", () => {
    expect(toAscii(layout(SHAPES.merge))).toBe(diagram`
      *┐  1
      *|  2a
      |*  2b
      *┘  3
      *  4
    `);
  });

  test("an unrelated branch head between a merge and its parents gets its own rail", () => {
    expect(toAscii(layout(SHAPES.unrelatedHead))).toBe(diagram`
      *┐  1
      ||*  2c
      *||  2a
      |*|  2b
      *┘|  3
      *┘  4
    `);
  });

  test("merging the next commit joins its existing rail", () => {
    expect(toAscii(layout(SHAPES.joinNext))).toBe(diagram`
      *  a1
      |*  b1
      *┤  a2
      |*  b2
      *|  a3
      *  b3
    `);
  });

  test("an octopus merge opens a rail per extra parent", () => {
    expect(toAscii(layout(SHAPES.octopus))).toBe(diagram`
      *┐┐  m
      *||  a
      |*|  b
      ||*  c
      *┘┘  r
    `);
  });

  test("disjoint histories keep separate rails", () => {
    expect(toAscii(layout(SHAPES.disjoint))).toBe(diagram`
      *  1
      |*  x
      *|  2
      *  y
    `);
  });

  test("criss-cross merges", () => {
    expect(toAscii(layout(SHAPES.crissCross))).toBe(diagram`
      *┐  m1
      ├|*  m2
      *||  a
      |*┘  b
      *┘  r
    `);
  });

  test("a merge whose second parent comes next but has no rail yet", () => {
    expect(toAscii(layout(SHAPES.mergeNextWithoutRail))).toBe(diagram`
      *  t
      *┐  m
      |*  f2
      *|  a
      |*  f1
      *┘  b
    `);
  });

  test("the WIP node sits on top of HEAD's rail", () => {
    expect(toAscii(layout(SHAPES.wip, { wip: WIP }))).toBe(diagram`
      *  WIP
      |*  1
      *┘  2
      *  3
    `);
  });
});
