import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, type FileEntry } from "../../ipc";
import { onFilesSignal } from "../../store/store";
import type { ListingErrors, Listings } from "./filesTree";

const MAX_LOADS = 4;

export interface ListingsState {
  listings: Listings;
  /** Why the plane root couldn't be listed, if it couldn't. */
  error: string | null;
  /** Why other folders couldn't be listed. */
  errors: ListingErrors;
}

/**
 * One plane's folder listings, keyed `""` for the root: the root plus every
 * folder in `wanted`, re-listed on refresh and on watcher reports. Mount it
 * per plane.
 */
export function useListings(planeId: string, wanted: readonly string[]): ListingsState {
  const [listings, setListings] = useState<Listings>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [errors, setErrors] = useState<ListingErrors>(new Map());
  const listed = useRef(new Set<string>());
  const wantedRef = useRef(wanted);
  wantedRef.current = wanted;
  const queue = useMemo(() => limiter(MAX_LOADS), []);

  const load = useCallback(
    (dir: string) => {
      listed.current.add(dir);
      void queue(() => commands.planeListDir(planeId, dir)).then(
        (entries: FileEntry[]) => {
          setListings((previous) => new Map(previous).set(dir, entries));
          setErrors((previous) => without(previous, dir));
          if (dir === "") setError(null);
        },
        (reason: unknown) => {
          listed.current.delete(dir);
          setListings((previous) => without(previous, dir));
          if (dir === "") setError(String(reason));
          else setErrors((previous) => new Map(previous).set(dir, String(reason)));
        },
      );
    },
    [planeId, queue],
  );

  useEffect(() => {
    for (const dir of ["", ...wanted]) {
      if (!listed.current.has(dir)) load(dir);
    }
  }, [wanted, load]);

  useEffect(
    () =>
      onFilesSignal((signal) => {
        if (signal.planeId !== planeId) return;
        if (signal.dirs === "all") {
          for (const dir of new Set(["", ...listed.current, ...wantedRef.current])) load(dir);
          return;
        }
        for (const dir of signal.dirs) {
          if (listed.current.has(dir)) load(dir);
        }
      }),
    [planeId, load],
  );

  return { listings, error, errors };
}

function without<V>(map: ReadonlyMap<string, V>, key: string): ReadonlyMap<string, V> {
  if (!map.has(key)) return map;
  const next = new Map(map);
  next.delete(key);
  return next;
}

/** Run at most `max` tasks at once, the rest in order as slots free up. */
function limiter(max: number) {
  let running = 0;
  const waiting: Array<() => void> = [];
  return async <T>(task: () => Promise<T>): Promise<T> => {
    if (running >= max) await new Promise<void>((resolve) => waiting.push(resolve));
    else running++;
    try {
      return await task();
    } finally {
      const next = waiting.shift();
      if (next) next();
      else running--;
    }
  };
}
