import { comboFromEvent, detectPlatform, KEYMAP, matchShortcut } from "./keymap";

describe("matchShortcut", () => {
  it("maps macOS ⌘ shortcuts", () => {
    expect(matchShortcut("mac", key("KeyT", { metaKey: true }))).toEqual({ action: "newTab" });
    expect(matchShortcut("mac", key("Digit3", { metaKey: true }))).toEqual({
      action: "tabN",
      digit: 3,
    });
    expect(matchShortcut("mac", key("Digit2", { metaKey: true, altKey: true }))).toEqual({
      action: "planeN",
      digit: 2,
    });
    expect(matchShortcut("mac", key("BracketRight", { metaKey: true, shiftKey: true }))).toEqual({
      action: "nextTab",
    });
    expect(matchShortcut("mac", key("KeyR", { metaKey: true }))).toEqual({
      action: "blockReload",
    });
    expect(matchShortcut("mac", key("Digit0", { metaKey: true }))).toEqual({
      action: "zoomReset",
    });
    expect(matchShortcut("mac", key("KeyD", { metaKey: true }))).toEqual({ action: "toolPanel" });
  });

  it("maps Linux Ctrl+Shift and Ctrl+Alt shortcuts", () => {
    expect(matchShortcut("linux", key("KeyT", { ctrlKey: true, shiftKey: true }))).toEqual({
      action: "newTab",
    });
    expect(matchShortcut("linux", key("Digit4", { ctrlKey: true, altKey: true }))).toEqual({
      action: "tabN",
      digit: 4,
    });
    expect(matchShortcut("linux", key("PageDown", { ctrlKey: true }))).toEqual({
      action: "nextTab",
    });
    expect(matchShortcut("linux", key("F5"))).toEqual({ action: "blockReload" });
    expect(matchShortcut("linux", key("KeyD", { ctrlKey: true, shiftKey: true }))).toEqual({
      action: "toolPanel",
    });
  });

  it("leaves ⌘Q to the app menu, whose Quit asks first", () => {
    expect(matchShortcut("mac", key("KeyQ", { metaKey: true }))).toBeNull();
  });

  it.each([
    ["KeyC", { ctrlKey: true }],
    ["KeyD", { ctrlKey: true }],
    ["KeyR", { ctrlKey: true }],
    ["KeyT", { ctrlKey: true }],
    ["KeyO", { ctrlKey: true }],
    ["Escape", {}],
    ["Tab", { shiftKey: true }],
    ["KeyP", { altKey: true }],
    ["Digit1", { altKey: true }],
  ] as const)("passes %s %o through to the PTY on both platforms", (code, modifiers) => {
    expect(matchShortcut("mac", key(code, modifiers))).toBeNull();
    expect(matchShortcut("linux", key(code, modifiers))).toBeNull();
  });

  it("writes every combo in the order comboFromEvent produces", () => {
    const order = ["Ctrl", "Alt", "Shift", "Meta"];
    for (const combo of Object.values(KEYMAP).flatMap((shortcuts) =>
      shortcuts.flatMap((s) => s.combos),
    )) {
      const modifiers = combo.split("+").slice(0, -1);
      const sorted = [...modifiers].sort((a, b) => order.indexOf(a) - order.indexOf(b));
      expect(modifiers, combo).toEqual(sorted);
    }
  });

  it("never claims the same combo twice", () => {
    for (const shortcuts of Object.values(KEYMAP)) {
      const combos = shortcuts.flatMap((s) => s.combos);
      expect(new Set(combos).size).toBe(combos.length);
    }
  });
});

describe("comboFromEvent", () => {
  it("orders modifiers consistently", () => {
    expect(
      comboFromEvent(
        key("ArrowDown", { metaKey: true, altKey: true, ctrlKey: true, shiftKey: true }),
      ),
    ).toBe("Ctrl+Alt+Shift+Meta+ArrowDown");
  });
});

describe("detectPlatform", () => {
  it("treats anything but macOS as Linux", () => {
    expect(detectPlatform("MacIntel")).toBe("mac");
    expect(detectPlatform("Linux x86_64")).toBe("linux");
  });
});

function key(
  code: string,
  modifiers: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {},
) {
  return { code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...modifiers };
}
