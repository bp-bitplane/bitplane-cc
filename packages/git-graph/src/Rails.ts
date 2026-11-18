import type { GraphCommit, GraphNode, Wip } from "./createGraph";

export type RailsState = {
  rails: Rail[];
  hidden: Hidden[];
  nextRailId: number;
  nextHiddenId: number;
};

export type Rail = {
  id: RailId;
  next: string;
};

export type Hidden = {
  id: HiddenId;
  next: readonly string[];
};

declare const railBrand: unique symbol;
export type RailId = number & { [railBrand]: typeof railBrand };

declare const hiddenBrand: unique symbol;
export type HiddenId = number & { [hiddenBrand]: typeof hiddenBrand };

export class Rails<T extends GraphCommit> {
  private usedHashes = new Set<string>();
  public readonly state: RailsState = {
    rails: [],
    hidden: [],
    nextRailId: 0,
    nextHiddenId: 1_000_000, // Use a large number so the hidden ids do not overlap with the rail ids
  };

  public constructor(
    private ignoreMergeParentsOf: ReadonlySet<string>,
    private visibleTips: ReadonlySet<string> | undefined,
  ) {}

  add(commit: T | Wip, next?: T): GraphNode<T> | undefined {
    if ("hash" in commit && this.usedHashes.has(commit.hash)) {
      return undefined;
    }

    const children = [...this.getChildren(commit)];

    const rails = this.state.rails.map((r) => r.id);
    let position: RailId;
    let forks: RailId[] = [];
    let merges: RailId[] = [];

    if (children.length === 0) {
      if ("hash" in commit && this.visibleTips && !this.visibleTips.has(commit.hash)) {
        // If the commit isn't in existing rail and isn't pointed to by a visible ref, hide it
        if (!this.isHidden(commit)) {
          this.state.hidden.push({
            id: this.newHiddenId(),
            next: commit.parents,
          });
        }

        return undefined;
      }

      // If the commit isn't in existing rail, create a new rail
      const rail = { next: commit.parents[0]!, id: this.newRailId() };

      this.state.rails.push(rail);

      merges = [rail.id];
      position = rail.id;
    } else {
      // If commit does have any child assign it to first child rail and mark rest as forks
      let first: RailId;
      [first, ...forks] = children as [RailId, ...RailId[]];

      const newRails: Rail[] = [];

      for (const r of this.state.rails) {
        if (r.id === first) {
          newRails.push({ next: commit.parents[0]!, id: first });
          // Discard forked rails
        } else if (!forks.includes(r.id)) {
          newRails.push(r);
        }
      }
      this.state.rails = newRails;
      position = first;
    }

    if (
      commit.parents.length > 1 &&
      "hash" in commit &&
      !this.ignoreMergeParentsOf.has(commit.hash)
    ) {
      const [, ...mergedParents] = [...commit.parents];
      for (const parent of mergedParents) {
        if (next && parent === next.hash) {
          const children = [...this.getChildren(next)];
          if (children.length === 1) {
            merges.push(children[0]!);
            continue;
          }
        }
        const rail = { next: parent, id: this.newRailId() };
        this.state.rails.push(rail);
        merges.push(rail.id);
      }
    }

    if (commit.parents.length === 0) {
      merges = [];
      this.state.rails = this.state.rails.filter((r) => r.id !== position);
      if (children.length > 0) {
        forks.push(position);
      }
    }

    if ("hash" in commit) {
      this.usedHashes.add(commit.hash);
    }

    const node: GraphNode<T> = {
      commit,
      position,
      rails,
      forks,
      merges,
    };

    if (process.env.NODE_ENV !== "production") {
      assetsNodeStructure(node);
    }

    return node;
  }

  private newRailId() {
    return this.state.nextRailId++ as RailId;
  }

  private newHiddenId() {
    return this.state.nextHiddenId++ as HiddenId;
  }

  private *getChildren(commit: T | Wip) {
    if (!("hash" in commit)) return;

    for (const r of this.state.rails) {
      if (r.next === commit.hash) {
        yield r.id;
      }
    }
  }

  private isHidden(commit: T | Wip) {
    if (!("hash" in commit)) return;

    for (const h of this.state.hidden) {
      if (h.next.includes(commit.hash)) {
        return true;
      }
    }
  }
}

const assetsNodeStructure = (node: GraphNode) => {
  if (node.forks.map((f) => node.rails.includes(f)).includes(false)) {
    console.error(node);
    throw new Error("forks is not subset of rails");
  }

  if (node.merges.filter((f) => node.rails.includes(f)).length > 1) {
    console.error(node);
    throw new Error("merges includes values of rails");
  }
};
