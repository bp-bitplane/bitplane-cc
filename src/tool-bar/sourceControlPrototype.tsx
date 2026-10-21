/**
 * PROTOTYPE (issue #36, "Source control readability"): throwaway variant switch.
 * Three variants of the Source control rows and Member separation, switchable via
 * `?sc=a|b|c|current`, the floating bar at the bottom, or ←/→ (dev builds only).
 * The choice is kept in localStorage, so it survives `pnpm dev` reloads.
 */
import { useEffect, useSyncExternalStore } from "react";

export const VARIANTS = [
  { key: "current", label: "Current (baseline)" },
  { key: "a", label: "A · Sidebar rows, spaced Members" },
  { key: "b", label: "B · Two-line rows, divided Members" },
  { key: "c", label: "C · Path-first rows, boxed Members" },
] as const;

export type Variant = (typeof VARIANTS)[number]["key"];

export function useScVariant(): Variant {
  return useSyncExternalStore(subscribe, () => current);
}

export function PrototypeSwitcher() {
  const variant = useScVariant();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      if (e.metaKey || e.ctrlKey || e.altKey || isTyping(e.target)) return;
      step(e.key === "ArrowLeft" ? -1 : 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  if (!import.meta.env.DEV) return null;
  const label = VARIANTS.find((v) => v.key === variant)?.label;
  return (
    <div style={bar} role="toolbar" aria-label="Prototype variant">
      <button type="button" style={arrow} onClick={() => step(-1)} aria-label="Previous variant">
        ←
      </button>
      <span style={{ padding: "0 8px", whiteSpace: "nowrap" }}>
        <b>SC prototype</b> · {label}
      </span>
      <button type="button" style={arrow} onClick={() => step(1)} aria-label="Next variant">
        →
      </button>
    </div>
  );
}

const STORAGE_KEY = "prototype.sourceControlVariant";
const listeners = new Set<() => void>();
let current: Variant = initial();

function initial(): Variant {
  const fromUrl = new URLSearchParams(window.location.search).get("sc");
  let stored: string | null = null;
  try {
    stored = localStorage.getItem(STORAGE_KEY);
  } catch {}
  return asVariant(fromUrl) ?? asVariant(stored) ?? "a";
}

function asVariant(value: string | null): Variant | null {
  return VARIANTS.find((v) => v.key === value?.toLowerCase())?.key ?? null;
}

function step(delta: number) {
  const index = VARIANTS.findIndex((v) => v.key === current);
  current = VARIANTS[(index + delta + VARIANTS.length) % VARIANTS.length]?.key ?? "a";
  try {
    localStorage.setItem(STORAGE_KEY, current);
  } catch {}
  const url = new URL(window.location.href);
  url.searchParams.set("sc", current);
  window.history.replaceState(null, "", url);
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || target.closest("input, textarea, [contenteditable]") !== null;
}

const bar: React.CSSProperties = {
  position: "fixed",
  left: "50%",
  bottom: 16,
  transform: "translateX(-50%)",
  zIndex: 1000,
  display: "flex",
  alignItems: "center",
  padding: 4,
  borderRadius: 999,
  background: "#ffe14d",
  color: "#111",
  font: "12px var(--sans)",
  boxShadow: "0 4px 16px rgba(0,0,0,0.5)",
};

const arrow: React.CSSProperties = {
  width: 26,
  height: 26,
  padding: 0,
  borderRadius: 999,
  background: "transparent",
  color: "#111",
  fontWeight: 700,
};
