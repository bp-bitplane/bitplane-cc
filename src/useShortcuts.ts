import { useEffect } from "react";
import { type Match, matchShortcut } from "./keymap";
import {
  cyclePlane,
  cycleTab,
  goToPlane,
  goToTab,
  openCreateDialog,
  openSession,
  refresh,
  requestCloseSession,
  requestQuit,
  togglePalette,
  useStore,
  zoom,
} from "./store/store";

/**
 * Catch app shortcuts in the capture phase, before wterm sees them; every
 * other key reaches the focused terminal untouched.
 */
export function useShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const match = matchShortcut(useStore.getState().platform, event);
      if (!match) return;
      event.preventDefault();
      event.stopPropagation();
      run(match);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}

export function run({ action, digit = 1 }: Match): void {
  switch (action) {
    case "newTab":
      void openSession();
      break;
    case "closeTab":
      void requestCloseSession();
      break;
    case "nextTab":
      cycleTab(1);
      break;
    case "prevTab":
      cycleTab(-1);
      break;
    case "tabN":
      goToTab(digit);
      break;
    case "nextPlane":
      cyclePlane(1);
      break;
    case "prevPlane":
      cyclePlane(-1);
      break;
    case "planeN":
      goToPlane(digit);
      break;
    case "palette":
      togglePalette();
      break;
    case "createPlane":
      openCreateDialog();
      break;
    case "refresh":
      void refresh();
      break;
    case "zoomIn":
      zoom(1);
      break;
    case "zoomOut":
      zoom(-1);
      break;
    case "zoomReset":
      zoom(0);
      break;
    case "quit":
      void requestQuit();
      break;
    case "blockReload":
      break;
  }
}
