import type { ReactElement } from "react";
import type { RailId } from "../Rails";
import { type RailRow, renderEmptyRails } from "./renderRails";

test("renderEmptyRails - merge commit", () => {
  const data = {
    position: 1 as RailId,
    rails: [0, 1, 2] as RailId[],
    forks: [2] as RailId[],
    merges: [3] as RailId[],
  } satisfies RailRow;

  const result = renderEmptyRails(data, 20);

  const colors: string[] = result.props.children.map(
    (e: ReactElement<{ className: string }>) => e.props.className,
  );

  expect(colors).toEqual(["git-graph-color-0", "git-graph-color-1", "git-graph-color-3"]);
});

test("renderEmptyRails - merge commit 2", () => {
  const data = {
    position: 0 as RailId,
    rails: [0, 18, 19, 20] as RailId[],
    forks: [18] as RailId[],
    merges: [21] as RailId[],
  } satisfies RailRow;

  const result = renderEmptyRails(data, 20);

  const colors: string[] = result.props.children.map(
    (e: ReactElement<{ className: string }>) => e.props.className,
  );

  expect(colors).toEqual([
    "git-graph-color-0",
    "git-graph-color-5",
    "git-graph-color-6",
    "git-graph-color-0",
  ]);
});

test("renderEmptyRails - index", () => {
  const data = {
    position: 0 as RailId,
    rails: [] as RailId[],
    forks: [0] as RailId[],
    merges: [] as RailId[],
  } satisfies RailRow;

  const result = renderEmptyRails(data, 20);

  const colors: string[] = result.props.children.map(
    (e: ReactElement<{ className: string }>) => e.props.className,
  );

  expect(colors).toEqual(["git-graph-color-0"]);
});
