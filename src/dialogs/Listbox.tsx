import { type ChangeEvent, type KeyboardEvent, type ReactNode, useState } from "react";
import styles from "./Palette.module.css";

/**
 * A filter input over a listbox: typing selects the first enabled match,
 * ↑/↓ move between enabled ones, Enter picks. `filter` gets the trimmed,
 * lower-cased query.
 */
export function useListbox<T extends object>(
  filter: (needle: string) => T[],
  pick: (item: T) => void,
) {
  const [query, setQuery] = useState("");
  const matches = filter(needleOf(query));
  const [index, setIndex] = useState(() => step(matches, -1, 1));
  const choose = (item: T | undefined) => {
    if (item && !disabled(item)) pick(item);
  };
  const input = {
    value: query,
    onChange: (e: ChangeEvent<HTMLInputElement>) => {
      setQuery(e.target.value);
      setIndex(step(filter(needleOf(e.target.value)), -1, 1));
    },
    onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setIndex((i) => step(matches, i, 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setIndex((i) => step(matches, i, -1));
      } else if (e.key === "Enter") {
        choose(matches[index]);
      }
    },
  };
  return { matches, index, choose, input };
}

export function ListboxOption({
  selected,
  disabled = false,
  title,
  onPick,
  children,
}: {
  selected: boolean;
  disabled?: boolean;
  title?: string;
  onPick: () => void;
  children: ReactNode;
}) {
  return (
    <div
      role="option"
      tabIndex={-1}
      aria-selected={selected}
      aria-disabled={disabled}
      title={title}
      className={`${styles.row} ${selected ? styles.active : ""} ${disabled ? styles.disabled : ""}`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onPick}
      // Keys go to the filter input, which keeps focus.
      onKeyDown={undefined}
    >
      {children}
    </div>
  );
}

function needleOf(query: string): string {
  return query.trim().toLowerCase();
}

/** The next enabled item from `from` in `delta`'s direction; `from` itself when there's none. */
function step(items: object[], from: number, delta: 1 | -1): number {
  for (let i = from + delta; i >= 0 && i < items.length; i += delta) {
    const item = items[i];
    if (item && !disabled(item)) return i;
  }
  return Math.max(from, 0);
}

function disabled(item: object): boolean {
  return "disabled" in item && item.disabled === true;
}
