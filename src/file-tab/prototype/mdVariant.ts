/**
 * PROTOTYPE (#37 "Markdown preview behaviour"), throwaway: which Markdown
 * preview variant the File tab shows. Picked by `?md=A|B|C`, else
 * localStorage `prototype:md-variant`, else A. Switched at runtime by
 * `MdVariantSwitcher`.
 */
import { useSyncExternalStore } from "react";

export const MD_VARIANTS = {
  A: "Header toggle, Source by default, hidden in diffs",
  B: "Preview by default, eye icon, disabled in diffs",
  C: "One view switch, Preview also in diffs",
} as const;

export type MdVariant = keyof typeof MD_VARIANTS;

export const MD_VARIANT_KEYS = Object.keys(MD_VARIANTS) as MdVariant[];

export function useMdVariant(): MdVariant {
  return useSyncExternalStore(subscribe, read, () => "A");
}

export function setMdVariant(variant: MdVariant): void {
  try {
    localStorage.setItem(STORAGE_KEY, variant);
  } catch {}
  const url = new URL(window.location.href);
  if (url.searchParams.has("md")) {
    url.searchParams.set("md", variant);
    window.history.replaceState(null, "", url);
  }
  current = variant;
  for (const listener of listeners) listener();
}

/** `.md` / `.markdown`, case-insensitive. */
export function isMarkdown(path: string): boolean {
  return /\.(md|markdown)$/i.test(path);
}

const STORAGE_KEY = "prototype:md-variant";
const listeners = new Set<() => void>();
let current: MdVariant = initial();

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function read(): MdVariant {
  return current;
}

function initial(): MdVariant {
  const fromUrl = new URLSearchParams(window.location.search).get("md")?.toUpperCase();
  if (isVariant(fromUrl)) return fromUrl;
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (isVariant(stored)) return stored;
  } catch {}
  return "A";
}

function isVariant(value: string | null | undefined): value is MdVariant {
  return value != null && value in MD_VARIANTS;
}
