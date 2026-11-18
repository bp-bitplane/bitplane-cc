import { createGraph, type GraphCommit, type GraphOptions } from "../src/createGraph";

export const layout = (commits: Iterable<GraphCommit>, options: GraphOptions = {}) =>
  layoutInChunks([[...commits]], options);

/** Feeds the commits chunk by chunk, the way a paginated log arrives. */
export const layoutInChunks = (chunks: GraphCommit[][], options: GraphOptions = {}) => {
  const graph = createGraph(options);
  for (const chunk of chunks) graph.append(chunk);
  graph.end();
  return graph;
};

export const commit = (hash: string, parents: string[]): GraphCommit => ({ hash, parents });
