import { type RailId, Rails, type RailsState } from "./Rails";

export type GraphCommit = {
  hash: string;
  parents: readonly string[];
};

/** The synthetic node for uncommitted changes, drawn above its parents. */
export type Wip = {
  readonly wip: true;
  parents: readonly string[];
};

export type GraphNode<T extends GraphCommit = GraphCommit> = {
  commit: T | Wip;
  position: RailId;

  /** State of rails after this commit */
  rails: RailId[];

  /** Which rails from are being created or forked from an existing one. Always subset of rails. Unordered */
  forks: RailId[];

  /** Which rails are being merged to this commit. Should not contain ids from rails, except for merging directly previous commit. Unordered */
  merges: RailId[];
};

export type GraphOptions = {
  wip?: { parents: readonly string[] };
  /** Lay out only history reachable from these hashes; everything else is hidden. */
  visibleTips?: ReadonlySet<string>;
  /** Commits drawn without their merge rails, such as stashes. */
  ignoreMergeParentsOf?: ReadonlySet<string>;
};

export type GraphBuilder<T extends GraphCommit> = {
  readonly nodes: readonly GraphNode<T>[];
  readonly rails: RailsState;
  /**
   * Adds commits newest first, as `git log --date-order` emits them. The last commit of
   * each call is laid out by the next `append` or by `end`, which needs it to see what follows.
   */
  append(commits: Iterable<T>): void;
  end(): void;
};

export const createGraph = <T extends GraphCommit>(options: GraphOptions = {}): GraphBuilder<T> => {
  const rails = new Rails<T>(options.ignoreMergeParentsOf ?? new Set(), options.visibleTips);
  const nodes: GraphNode<T>[] = [];
  let pending: T | undefined;

  const add = (commit: T | Wip, next?: T) => {
    const node = rails.add(commit, next);
    if (node) nodes.push(node);
  };

  if (options.wip) add({ wip: true, parents: options.wip.parents });

  return {
    nodes,
    rails: rails.state,
    append(commits) {
      for (const commit of commits) {
        if (pending) add(pending, commit);
        pending = commit;
      }
    },
    end() {
      if (pending) add(pending);
      pending = undefined;
    },
  };
};

export const isWip = (commit: GraphCommit | Wip): commit is Wip => "wip" in commit;
