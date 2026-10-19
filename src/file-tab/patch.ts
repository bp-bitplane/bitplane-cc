/**
 * Reading a unified patch without the renderer, so the File tab can decide
 * what to show before `DiffView` (and `@pierre/diffs`) loads.
 */

/** Past this many added and removed lines, a diff waits for "Show anyway". */
export const MAX_CHANGED_LINES = 20_000;

export function changedLines(patch: string): number {
  let count = 0;
  let inHunk = false;
  for (const line of patch.split("\n")) {
    if (line.startsWith("@@")) inHunk = true;
    else if (line.startsWith("diff --git")) inHunk = false;
    else if (inHunk && (line.startsWith("+") || line.startsWith("-"))) count++;
  }
  return count;
}

/** The new-side line of the first change, for `code -g`; 1 for a pure rename. */
export function firstChangedLine(patch: string): number {
  let line = 0;
  for (const text of patch.split("\n")) {
    const hunk = /^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (hunk) {
      line = Number(hunk[1]);
      continue;
    }
    if (line === 0) continue;
    if (text.startsWith("+") || text.startsWith("-")) return Math.max(line, 1);
    if (text.startsWith(" ")) line++;
  }
  return 1;
}

export function renameOf(patch: string): { from: string; to: string } | null {
  const from = /^rename from (.+)$/m.exec(patch)?.[1];
  const to = /^rename to (.+)$/m.exec(patch)?.[1];
  return from && to ? { from, to } : null;
}
