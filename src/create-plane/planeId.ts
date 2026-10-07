/** bp's plane id rule: lowercase alphanumerics plus `.`, `_` and `-`, not starting with punctuation. */
const PLANE_ID = /^[a-z0-9][a-z0-9._-]*$/;
const MAX_LENGTH = 64;

/** Why `id` can't be used, or `null` when it can. */
export function planeIdProblem(id: string, existingIds: Iterable<string>): string | null {
  if (id === "") return "Enter an id";
  if (!PLANE_ID.test(id)) {
    return "Use lowercase letters, digits, '.', '_' and '-', starting with a letter or digit";
  }
  if (id.length > MAX_LENGTH) return `At most ${MAX_LENGTH} characters`;
  if (id.startsWith("bp-")) return "Ids starting with 'bp-' are reserved";
  for (const existing of existingIds) {
    if (existing === id) return `${id} already exists`;
  }
  return null;
}

/** The id prefilled from a branch: lowercased, `/` becomes `-`, no leading `bp-`. */
export function idFromBranch(branch: string): string {
  return branch
    .trim()
    .toLowerCase()
    .replaceAll("/", "-")
    .replace(/^(bp-)+/, "");
}
