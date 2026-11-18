import type { GraphCommit, GraphOptions } from "../../src/createGraph";
import { commit as c } from "../layout";

export const SHAPES = {
  linear: [c("1", ["2"]), c("2", ["3"]), c("3", [])],
  sideBranch: [c("1", ["2a"]), c("2a", ["3"]), c("2b", ["3"]), c("3", ["4"]), c("4", [])],
  merge: [c("1", ["2a", "2b"]), c("2a", ["3"]), c("2b", ["3"]), c("3", ["4"]), c("4", [])],
  unrelatedHead: [
    c("1", ["2a", "2b"]),
    c("2c", ["4"]),
    c("2a", ["3"]),
    c("2b", ["3"]),
    c("3", ["4"]),
    c("4", []),
  ],
  joinNext: [
    c("a1", ["a2"]),
    c("b1", ["b2"]),
    c("a2", ["a3", "b2"]),
    c("b2", ["b3"]),
    c("a3", []),
    c("b3", []),
  ],
  octopus: [c("m", ["a", "b", "c"]), c("a", ["r"]), c("b", ["r"]), c("c", ["r"]), c("r", [])],
  disjoint: [c("1", ["2"]), c("x", ["y"]), c("2", []), c("y", [])],
  crissCross: [c("m1", ["a", "b"]), c("m2", ["b", "a"]), c("a", ["r"]), c("b", ["r"]), c("r", [])],
  mergeNextWithoutRail: [
    c("t", ["m"]),
    c("m", ["a", "f2"]),
    c("f2", ["f1"]),
    c("a", ["b"]),
    c("f1", ["b"]),
    c("b", []),
  ],
  wip: [c("1", ["2"]), c("2", ["3"]), c("3", [])],
} satisfies Record<string, GraphCommit[]>;

export const WIP: GraphOptions["wip"] = { parents: ["2"] };
