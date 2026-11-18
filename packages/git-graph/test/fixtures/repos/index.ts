import { readFileSync } from "node:fs";
import type { GitCommit } from "../../../src/git";
import { commit } from "../../layout";

export const REPOS = ["bitplane-cc", "open-git-graph", "git"] as const;

export const loadRepo = (name: (typeof REPOS)[number]): GitCommit[] =>
  readFileSync(new URL(`./${name}.log`, import.meta.url), "utf8")
    .trim()
    .split("\n")
    .map((line) => {
      const [hash, ...parents] = line.trim().split(" ");
      return commit(hash!, parents);
    });
