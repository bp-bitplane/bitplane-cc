export type GitCommit = {
  hash: string;
  parents: string[];
  subject: string;
  author: string;
  authorEmail: string;
  authorDate: number;
  commitDate: number;
  files: GitCommitFile[];
  reflogSelector?: string;

  type: "commit" | "stash";
};

export type GitCommitFile = {
  path: string;
  mode: GitFileMode;
};

export type GitFileMode = "M" | "T" | "A" | "D" | "R" | "C" | "U" | "?" | "!";

export type GitIndex = {
  branch: string | undefined;
  parents: string[];
  tracked: GitCommitFile[];
  untracked: GitCommitFile[];
};
