/**
 * App shortcuts (spec §3). Combos are built from `KeyboardEvent.code`, so they
 * don't depend on the keyboard layout.
 *
 * On macOS every app shortcut uses ⌘, which a terminal never needs. Elsewhere
 * they use Ctrl+Shift or Ctrl+Alt, the terminal-emulator convention, so plain
 * Ctrl+letter, Alt+letter, Esc and Shift+Tab always reach the PTY.
 */

export type Platform = "mac" | "linux";

export type Action =
  | "newTab"
  | "closeTab"
  | "nextTab"
  | "prevTab"
  | "tabN"
  | "nextPlane"
  | "prevPlane"
  | "planeN"
  | "palette"
  | "createPlane"
  | "refresh"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "quit"
  | "blockReload";

export interface Shortcut {
  action: Action;
  combos: string[];
  label: string;
}

export interface Match {
  action: Action;
  /** 1–9 for `tabN` and `planeN`. */
  digit?: number;
}

export const KEYMAP: Record<Platform, Shortcut[]> = {
  mac: [
    { action: "newTab", combos: ["Meta+KeyT"], label: "⌘T" },
    { action: "closeTab", combos: ["Meta+KeyW"], label: "⌘W" },
    { action: "nextTab", combos: ["Shift+Meta+BracketRight", "Ctrl+Tab"], label: "⌘⇧]" },
    { action: "prevTab", combos: ["Shift+Meta+BracketLeft", "Ctrl+Shift+Tab"], label: "⌘⇧[" },
    { action: "tabN", combos: digits("Meta"), label: "⌘1…9" },
    { action: "nextPlane", combos: ["Alt+Meta+ArrowDown"], label: "⌘⌥↓" },
    { action: "prevPlane", combos: ["Alt+Meta+ArrowUp"], label: "⌘⌥↑" },
    { action: "planeN", combos: digits("Alt+Meta"), label: "⌘⌥1…9" },
    { action: "palette", combos: ["Meta+KeyP"], label: "⌘P" },
    { action: "createPlane", combos: ["Shift+Meta+KeyN"], label: "⌘⇧N" },
    { action: "refresh", combos: ["Shift+Meta+KeyR"], label: "⌘⇧R" },
    { action: "zoomIn", combos: ["Meta+Equal", "Shift+Meta+Equal"], label: "⌘=" },
    { action: "zoomOut", combos: ["Meta+Minus"], label: "⌘-" },
    { action: "zoomReset", combos: ["Meta+Digit0"], label: "⌘0" },
    { action: "quit", combos: ["Meta+KeyQ"], label: "⌘Q" },
    { action: "blockReload", combos: ["Meta+KeyR"], label: "⌘R" },
  ],
  linux: [
    { action: "newTab", combos: ["Ctrl+Shift+KeyT"], label: "Ctrl+Shift+T" },
    { action: "closeTab", combos: ["Ctrl+Shift+KeyW"], label: "Ctrl+Shift+W" },
    { action: "nextTab", combos: ["Ctrl+Tab", "Ctrl+PageDown"], label: "Ctrl+Tab" },
    { action: "prevTab", combos: ["Ctrl+Shift+Tab", "Ctrl+PageUp"], label: "Ctrl+Shift+Tab" },
    { action: "tabN", combos: digits("Ctrl+Alt"), label: "Ctrl+Alt+1…9" },
    { action: "nextPlane", combos: ["Ctrl+Shift+ArrowDown"], label: "Ctrl+Shift+↓" },
    { action: "prevPlane", combos: ["Ctrl+Shift+ArrowUp"], label: "Ctrl+Shift+↑" },
    { action: "planeN", combos: digits("Ctrl+Shift"), label: "Ctrl+Shift+1…9" },
    { action: "palette", combos: ["Ctrl+Shift+KeyP"], label: "Ctrl+Shift+P" },
    { action: "createPlane", combos: ["Ctrl+Shift+KeyN"], label: "Ctrl+Shift+N" },
    { action: "refresh", combos: ["Ctrl+Shift+KeyR"], label: "Ctrl+Shift+R" },
    { action: "zoomIn", combos: ["Ctrl+Shift+Equal"], label: "Ctrl+Shift+=" },
    { action: "zoomOut", combos: ["Ctrl+Shift+Minus"], label: "Ctrl+Shift+-" },
    { action: "zoomReset", combos: ["Ctrl+Shift+Digit0"], label: "Ctrl+Shift+0" },
    { action: "blockReload", combos: ["F5"], label: "F5" },
  ],
};

export function detectPlatform(platform = navigator.platform): Platform {
  return platform.startsWith("Mac") ? "mac" : "linux";
}

/** The shortcut an event triggers, or `null` when the key belongs to the PTY. */
export function matchShortcut(
  platform: Platform,
  event: Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "code">,
): Match | null {
  const combo = comboFromEvent(event);
  for (const shortcut of KEYMAP[platform]) {
    if (!shortcut.combos.includes(combo)) continue;
    if (shortcut.action === "tabN" || shortcut.action === "planeN") {
      return { action: shortcut.action, digit: Number(combo.slice(-1)) };
    }
    return { action: shortcut.action };
  }
  return null;
}

export function labelFor(platform: Platform, action: Action): string {
  return KEYMAP[platform].find((shortcut) => shortcut.action === action)?.label ?? "";
}

/** Modifiers in a fixed order: Ctrl, Alt, Shift, Meta, then the code. */
export function comboFromEvent(
  event: Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "code">,
): string {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");
  parts.push(event.code);
  return parts.join("+");
}

function digits(prefix: string): string[] {
  return Array.from({ length: 9 }, (_, i) => `${prefix}+Digit${i + 1}`);
}
