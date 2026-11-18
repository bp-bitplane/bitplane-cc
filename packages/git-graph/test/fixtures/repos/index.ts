import { readFileSync } from "node:fs";
import type { GitCommit } from "../../../src/git";
import { parseLog } from "./parse";

export const REPOS = ["bitplane-cc", "open-git-graph", "git"] as const;

export const loadRepo = (name: (typeof REPOS)[number]): GitCommit[] =>
  parseLog(readFileSync(new URL(`./${name}.log`, import.meta.url), "utf8"));
