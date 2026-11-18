import { readFileSync } from "node:fs";
import type { GraphCommit } from "../../../src/createGraph";
import { parseLog } from "./parse";

export const REPOS = ["bitplane-cc", "open-git-graph", "git"] as const;

export const loadRepo = (name: (typeof REPOS)[number]): GraphCommit[] =>
  parseLog(readFileSync(new URL(`./${name}.log`, import.meta.url), "utf8"));
