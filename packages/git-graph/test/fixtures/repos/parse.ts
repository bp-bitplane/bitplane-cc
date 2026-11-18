import type { GitCommit } from "../../../src/git";
import { commit } from "../../layout";

export const parseLog = (log: string): GitCommit[] =>
  log
    .trim()
    .split("\n")
    .map((line) => {
      const [hash, ...parents] = line.trim().split(" ");
      return commit(hash!, parents);
    });
