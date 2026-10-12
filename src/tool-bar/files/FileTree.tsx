/**
 * The only place that touches `@pierre/trees` (ADR 0007): swapping the
 * library means rewriting this file, not its callers.
 */
import type { ContextMenuItem, FileTreeBatchOperation } from "@pierre/trees";
import { FileTree as PierreTree, useFileTree } from "@pierre/trees/react";
import { type KeyboardEvent, type MouseEvent, useEffect, useRef } from "react";
import styles from "./FileTree.module.css";
import type { FileStatusEntry } from "./filesTree";

export interface MenuAction {
  label: string;
  run: () => void;
}

export interface FileTreeProps {
  /** Folders end with "/". */
  paths: readonly string[];
  folders: readonly string[];
  statuses: readonly FileStatusEntry[];
  /** Folder → text shown at the end of its row. */
  decorations: ReadonlyMap<string, string>;
  /** The folders that should be open; the tree reports changes through `onExpandedChange`. */
  expanded: readonly string[];
  onExpandedChange: (expanded: string[]) => void;
  /** Double-click or Enter on a file. */
  onOpen: (path: string) => void;
  menu: (path: string) => MenuAction[];
}

export function FileTree({
  paths,
  folders,
  statuses,
  decorations,
  expanded,
  onExpandedChange,
  onOpen,
  menu,
}: FileTreeProps) {
  const decorationsRef = useRef(decorations);
  const { model } = useFileTree({
    paths: [],
    density: "compact",
    flattenEmptyDirectories: true,
    initialExpansion: "closed",
    search: false,
    composition: { contextMenu: { enabled: true, triggerMode: "right-click" } },
    renderRowDecoration: ({ item }) => {
      const text = decorationsRef.current.get(item.path);
      return text ? { text, title: text } : null;
    },
  });

  const shown = useRef<readonly string[]>([]);
  const syncing = useRef(false);
  const latest = useRef({ folders, expanded, onExpandedChange });
  latest.current = { folders, expanded, onExpandedChange };

  useEffect(() => {
    syncing.current = true;
    model.batch(pathUpdates(shown.current, paths));
    shown.current = paths;
    applyExpanded(model, folders, expanded);
    syncing.current = false;
  }, [model, paths, folders, expanded]);

  useEffect(() => model.setGitStatus(statuses), [model, statuses]);

  useEffect(() => {
    if (decorationsRef.current === decorations) return;
    decorationsRef.current = decorations;
    model.setComposition(model.getComposition());
  }, [model, decorations]);

  useEffect(
    () =>
      model.subscribe(() => {
        if (syncing.current) return;
        const { folders, expanded, onExpandedChange } = latest.current;
        const next = readExpanded(model, folders, expanded);
        if (!sameSet(next, expanded)) onExpandedChange(next);
      }),
    [model],
  );

  const openFrom = (path: string | null) => {
    if (path && !path.endsWith("/") && model.getItem(path)?.isDirectory() === false) onOpen(path);
  };
  const onDoubleClick = (event: MouseEvent) => openFrom(rowPath(event.nativeEvent));
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Enter") openFrom(model.getFocusedPath());
  };

  return (
    <PierreTree
      model={model}
      className={styles.tree}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKeyDown}
      renderContextMenu={(item: ContextMenuItem, context) => {
        const actions = menu(item.path);
        if (actions.length === 0) return null;
        return (
          <div className={styles.menu} role="menu" data-file-tree-context-menu-root="true">
            {actions.map((action) => (
              <button
                key={action.label}
                type="button"
                role="menuitem"
                className={styles.menuItem}
                onClick={() => {
                  action.run();
                  context.close();
                }}
              >
                {action.label}
              </button>
            ))}
          </div>
        );
      }}
    />
  );
}

type Model = ReturnType<typeof useFileTree>["model"];

/** Removals before additions, shallowest first, so a removed folder takes its contents along. */
export function pathUpdates(
  previous: readonly string[],
  next: readonly string[],
): FileTreeBatchOperation[] {
  const before = new Set(previous);
  const after = new Set(next);
  const updates: FileTreeBatchOperation[] = [];
  const removedFolders: string[] = [];
  for (const path of previous.filter((p) => !after.has(p)).sort(byDepth)) {
    if (removedFolders.some((folder) => path.startsWith(folder))) continue;
    const recursive = path.endsWith("/");
    updates.push({ type: "remove", path, ...(recursive && { recursive }) });
    if (recursive) removedFolders.push(path);
  }
  for (const path of next.filter((p) => !before.has(p)).sort(byDepth)) {
    updates.push({ type: "add", path });
  }
  return updates;
}

function applyExpanded(model: Model, folders: readonly string[], expanded: readonly string[]) {
  const open = new Set(expanded);
  for (const folder of folders) {
    const item = model.getItem(folder);
    if (!item || !("isExpanded" in item)) continue;
    if (open.has(folder) && !item.isExpanded()) item.expand();
    else if (!open.has(folder) && item.isExpanded()) item.collapse();
  }
}

/** What the tree has open, keeping wanted folders it doesn't show yet. */
function readExpanded(model: Model, folders: readonly string[], expanded: readonly string[]) {
  const shown = new Set(folders);
  const open = expanded.filter((folder) => !shown.has(folder));
  for (const folder of folders) {
    const item = model.getItem(folder);
    if (item && "isExpanded" in item && item.isExpanded()) open.push(folder);
  }
  return open;
}

function rowPath(event: Event): string | null {
  for (const target of event.composedPath()) {
    if (target instanceof HTMLElement && target.dataset.itemPath) return target.dataset.itemPath;
  }
  return null;
}

function sameSet(a: readonly string[], b: readonly string[]) {
  const set = new Set(b);
  return a.length === b.length && a.every((x) => set.has(x));
}

function byDepth(a: string, b: string) {
  return a.split("/").length - b.split("/").length;
}
