import type { BranchIntent } from "../ipc";

export const DEFAULT_BRANCH_INTENT: BranchIntent = "resolve";

/** bp refuses `--no-fetch` unless the branch is required new or existing. */
export function allowsNoFetch(intent: BranchIntent): boolean {
  return intent !== "resolve";
}
