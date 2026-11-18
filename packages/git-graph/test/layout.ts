import { createGraphNodes, type Graph } from "../src/createGraphNodes";
import type { GitCommit, GitIndex } from "../src/git";

export type LayoutOptions = {
  index?: GitIndex;
  stashes?: GitCommit[];
  visibleTips?: Set<string>;
};

/** Lays out all commits at once; the one place tests touch the layout API. */
export const layout = (commits: Iterable<GitCommit>, options: LayoutOptions = {}): Graph =>
  createGraphNodes(commits, options.index, options.stashes ?? [], options.visibleTips ?? new Set())
    .toArray()
    .at(-1)!;

export const commit = (hash: string, parents: string[], subject = hash): GitCommit => ({
  hash,
  parents,
  subject,
  author: "",
  authorDate: 0,
  commitDate: 0,
  authorEmail: "",
  files: [],
  type: "commit",
});
