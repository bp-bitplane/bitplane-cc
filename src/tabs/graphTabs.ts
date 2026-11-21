// PROTOTYPE (alfonz/proto-git-graph): a third Tab kind showing one repo's git graph.

export interface GraphTab {
  planeId: string;
  /** Absolute, as Source control's `Repo.path`. */
  repoPath: string;
  name: string;
}

export type GraphTabKey = `graph:${string}`;

export function graphTabKey(tab: Pick<GraphTab, "repoPath">): GraphTabKey {
  return `graph:${tab.repoPath}`;
}
