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

/** Feeds the commits chunk by chunk, the way a paginated log arrives. */
export const layoutInChunks = (chunks: GitCommit[][], options: LayoutOptions = {}): Graph => {
  const [first = [], ...rest] = chunks;
  const graphs = createGraphNodes(
    first,
    options.index,
    options.stashes ?? [],
    options.visibleTips ?? new Set(),
  );
  let graph = graphs.next().value!;
  for (const chunk of rest) graph = graphs.next(chunk).value!;
  return graph;
};

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
