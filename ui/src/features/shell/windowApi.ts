import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

import { transport } from "../../lib/transport";

/** The window operations the title bar needs. Injectable so the chrome can be tested. */
export interface WindowApi {
  minimize: () => Promise<void>;
  toggleMaximize: () => Promise<void>;
  close: () => Promise<void>;
  isMaximized: () => Promise<boolean>;
  /** Calls `handler` whenever the window is resized (including maximize and restore). */
  onResized: (handler: () => void) => Promise<() => void>;
}

/** The current Tauri window, or `null` when the UI runs in a browser. */
export function tauriWindowApi(): WindowApi | null {
  if (transport() !== "desktop") {
    return null;
  }
  const current = getCurrentWindow();
  return {
    minimize: () => current.minimize(),
    toggleMaximize: () => current.toggleMaximize(),
    close: () => current.close(),
    isMaximized: () => current.isMaximized(),
    onResized: (handler) => current.onResized(handler),
  };
}

/** Whether the window is maximized, kept in sync while it is resized. */
export function useWindowMaximized(api: WindowApi | null): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!api) {
      return;
    }
    let active = true;
    let unlisten: (() => void) | undefined;
    const refresh = () => {
      api
        .isMaximized()
        .then((value) => {
          if (active) {
            setMaximized(value);
          }
        })
        .catch(() => {});
    };
    refresh();
    api
      .onResized(refresh)
      .then((stop) => {
        if (active) {
          unlisten = stop;
        } else {
          stop();
        }
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, [api]);

  return maximized;
}
