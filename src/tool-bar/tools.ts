export const TOOLS = ["sourceControl", "files"] as const;

export type Tool = (typeof TOOLS)[number];

export const TOOL_LABELS: Record<Tool, string> = {
  sourceControl: "Source control",
  files: "Files",
};
