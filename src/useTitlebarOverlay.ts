import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { useStore } from "./store/store";

/** True while macOS draws its traffic lights over the app; they hide in fullscreen. */
export function useTitlebarOverlay(): boolean {
  const mac = useStore((s) => s.platform === "mac");
  const [fullscreen, setFullscreen] = useState(false);

  useEffect(() => {
    if (!mac) return;
    const window = getCurrentWindow();
    const sync = () => void window.isFullscreen().then(setFullscreen);
    sync();
    const unlisten = window.onResized(sync);
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [mac]);

  return mac && !fullscreen;
}
