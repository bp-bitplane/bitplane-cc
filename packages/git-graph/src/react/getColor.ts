import type { RailId } from "../Rails";

export const COLOR_COUNT = 7;
export const getColor = (x: RailId) => `git-graph-color-${x % COLOR_COUNT}`;
