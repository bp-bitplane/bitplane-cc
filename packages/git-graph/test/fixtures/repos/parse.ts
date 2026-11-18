import type { GraphCommit } from "../../../src/createGraph";
import { commit } from "../../layout";

export const parseLog = (log: string): GraphCommit[] =>
  log
    .trim()
    .split("\n")
    .map((line) => {
      const [hash, ...parents] = line.trim().split(" ");
      return commit(hash!, parents);
    });
