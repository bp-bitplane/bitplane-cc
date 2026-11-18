import { type GraphNode, isWip } from "../src/createGraph";

/**
 * One line per node, one column per rail entering the row from above:
 * `*` commit, `|` rail passing by, `┘`/`└` rail ending in the commit,
 * `┐` rail the commit opens to the right, `┤`/`├` merge into a rail already there.
 */
export const toAscii = (graph: { nodes: readonly GraphNode[] }): string =>
  graph.nodes.map((n) => `${row(n)}  ${label(n)}`.trimEnd()).join("\n");

const row = (n: GraphNode): string => {
  const cells = n.rails.map((r) => (n.forks.includes(r) ? "fork" : "pass"));
  const at = n.rails.indexOf(n.position);
  const commitColumn = at === -1 ? n.rails.length : at;

  const chars = cells.map((cell, x) => {
    if (x === commitColumn) return "*";
    if (cell === "fork") return x < commitColumn ? "└" : "┘";
    return n.merges.includes(n.rails[x]!) ? (x < commitColumn ? "├" : "┤") : "|";
  });
  if (commitColumn === n.rails.length) chars.push("*");

  const opened = n.merges.filter((m) => !n.rails.includes(m) && m !== n.position);
  return [...chars, ...opened.map(() => "┐")].join("");
};

const label = (n: GraphNode): string => (isWip(n.commit) ? "WIP" : n.commit.hash);

/** Strips the common indentation so expected diagrams can be indented with the test. */
export const diagram = (strings: TemplateStringsArray): string => {
  const lines = strings.join("").split("\n").slice(1, -1);
  const indent = Math.min(...lines.filter((l) => l.trim()).map((l) => l.search(/\S/)));
  return lines.map((l) => l.slice(indent)).join("\n");
};
