/**
 * File tabs (ADR 0008): a frontend-only Tab kind showing one file of a plane,
 * as a diff or plain contents. Never saved, never restored.
 */

/**
 * What a File tab compares: `index` is HEAD↔index, `worktree` index↔worktree
 * (both from Source control), `file` HEAD↔worktree, plain when unchanged.
 */
export type FileTabMode = "index" | "worktree" | "file";

export interface FileTab {
  planeId: string;
  /** Relative to the plane root, `/`-separated. */
  path: string;
  mode: FileTabMode;
  /** An unpinned File tab is the plane's preview, replaced by the next open. */
  pinned: boolean;
}

/** A File tab's identity within its plane; never a number, so never a session id. */
export type FileTabKey = `${FileTabMode}:${string}`;

export function fileTabKey(tab: Pick<FileTab, "path" | "mode">): FileTabKey {
  return `${tab.mode}:${tab.path}`;
}

export function fileTabTitle(tab: Pick<FileTab, "path" | "mode">): string {
  const name = baseName(tab.path);
  switch (tab.mode) {
    case "index":
      return `${name} (Index)`;
    case "worktree":
      return `${name} (Working Tree)`;
    case "file":
      return name;
  }
}

/** `""` for the plane root. */
export function folderOf(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash === -1 ? "" : path.slice(0, slash);
}

/**
 * Key → the shortest trailing folder that tells apart File tabs whose file
 * names collide; tabs with a unique name get none.
 */
export function folderSuffixes(tabs: FileTab[]): Map<FileTabKey, string> {
  const pathsByName = new Map<string, Set<string>>();
  for (const tab of tabs) {
    const name = baseName(tab.path);
    pathsByName.set(name, (pathsByName.get(name) ?? new Set()).add(tab.path));
  }
  const suffixes = new Map<FileTabKey, string>();
  for (const tab of tabs) {
    const paths = pathsByName.get(baseName(tab.path));
    if (!paths || paths.size < 2) continue;
    suffixes.set(fileTabKey(tab), distinctFolder(tab.path, [...paths]));
  }
  return suffixes;
}

function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

function distinctFolder(path: string, colliding: string[]): string {
  const folder = folderOf(path);
  if (folder === "") return "./";
  const segments = folder.split("/");
  for (let n = 1; n < segments.length; n++) {
    const tail = trailing(folder, n);
    const clash = colliding.some(
      (other) => other !== path && trailing(folderOf(other), n) === tail,
    );
    if (!clash) return tail;
  }
  return folder;
}

function trailing(folder: string, n: number): string {
  return folder.split("/").slice(-n).join("/");
}
