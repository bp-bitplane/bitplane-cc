/**
 * Files' data as the tree sees it. Tree paths are relative to the plane root,
 * folders end with "/"; a folder's listing key is the same path without it.
 */
import type { ChangeKind, FileEntry, Head, Repo } from "../../ipc";

export type Listings = ReadonlyMap<string, readonly FileEntry[]>;
/** Listing key → why that folder couldn't be listed. */
export type ListingErrors = ReadonlyMap<string, string>;

export const ERROR_ROW = "Couldn't list this folder";

export type FileStatus = "added" | "deleted" | "ignored" | "modified" | "renamed" | "untracked";

export interface FileStatusEntry {
  path: string;
  status: FileStatus;
}

export interface TreeData {
  paths: string[];
  /** Every listed folder, as tree paths. */
  folders: string[];
  ignored: FileStatusEntry[];
  /** Placeholder rows standing in for folders that couldn't be listed → why. */
  errors: Map<string, string>;
}

/** Everything reachable from the root through listed folders. */
export function treeData(listings: Listings, errors: ListingErrors = new Map()): TreeData {
  const data: TreeData = { paths: [], folders: [], ignored: [], errors: new Map() };
  const visit = (dir: string) => {
    const error = errors.get(dir);
    if (error !== undefined && !listings.has(dir)) {
      const path = `${dir}/${ERROR_ROW}`;
      data.paths.push(path);
      data.errors.set(path, error);
    }
    for (const entry of listings.get(dir) ?? []) {
      const path = dir ? `${dir}/${entry.name}` : entry.name;
      const treePath = entry.kind === "directory" ? `${path}/` : path;
      data.paths.push(treePath);
      if (entry.kind === "directory") data.folders.push(treePath);
      if (entry.ignored) data.ignored.push({ path: treePath, status: "ignored" });
      if (entry.kind === "directory") visit(path);
    }
  };
  visit("");
  return data;
}

export function listingKey(treePath: string): string {
  return treePath.replace(/\/$/, "");
}

/** Git changes in plane-relative tree paths, ahead of anything ignored. */
export function fileStatuses(
  repos: readonly Repo[],
  planeDir: string,
  ignored: readonly FileStatusEntry[] = [],
): FileStatusEntry[] {
  const statuses = new Map<string, FileStatus>(ignored.map((e) => [e.path, e.status]));
  for (const repo of repos) {
    if (repo.state.kind !== "ready") continue;
    const prefix = repoPrefix(repo, planeDir);
    if (prefix === null) continue;
    const { staged, unstaged, untracked } = repo.state.status;
    for (const change of [...staged, ...unstaged]) {
      statuses.set(prefix + change.path, STATUS_OF[change.kind]);
    }
    for (const path of untracked) statuses.set(prefix + path, "untracked");
  }
  return [...statuses].map(([path, status]) => ({ path, status }));
}

/** Each repo's folder → its branch, or short SHA when detached. */
export function repoBranches(repos: readonly Repo[], planeDir: string): Map<string, string> {
  const branches = new Map<string, string>();
  for (const repo of repos) {
    const prefix = repoPrefix(repo, planeDir);
    if (!prefix || repo.state.kind !== "ready") continue;
    branches.set(prefix, headLabel(repo.state.status.head));
  }
  return branches;
}

/** The folders leading to each repo, so Files opens on the Members, each one collapsed. */
export function initialExpanded(repos: readonly Repo[], planeDir: string): string[] {
  const expanded = new Set<string>();
  for (const repo of repos) {
    const prefix = repoPrefix(repo, planeDir);
    if (!prefix) continue;
    const segments = prefix.split("/").filter(Boolean);
    for (let i = 1; i < segments.length; i++) expanded.add(`${segments.slice(0, i).join("/")}/`);
  }
  return [...expanded];
}

export function headLabel(head: Head): string {
  return head.kind === "branch" ? head.name : head.sha;
}

const STATUS_OF: Record<ChangeKind, FileStatus> = {
  modified: "modified",
  typeChanged: "modified",
  conflicted: "modified",
  added: "added",
  copied: "added",
  deleted: "deleted",
  renamed: "renamed",
};

/** The repo's folder as a tree path prefix: "" at the plane root, else "dir/". */
export function repoPrefix(repo: Repo, planeDir: string): string | null {
  const root = planeDir.replace(/\/$/, "");
  if (repo.path === root) return "";
  if (!repo.path.startsWith(`${root}/`)) return null;
  return `${repo.path.slice(root.length + 1)}/`;
}
