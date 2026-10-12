export const TOOLS = ["sourceControl"] as const;

export type Tool = (typeof TOOLS)[number];

export const TOOL_LABELS: Record<Tool, string> = {
  sourceControl: "Source control",
};
