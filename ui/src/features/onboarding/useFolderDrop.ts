import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

/**
 * Folders dropped on the desktop window arrive as the window's drag-and-drop event with their
 * paths. Returns whether something is being dragged over the window, for the drop zone's
 * highlight. A plain browser gives no paths, so there the drop zone is only a button.
 */
export function useFolderDrop(onPaths: (paths: string[]) => void): boolean {
  const [over, setOver] = useState(false);
  const handler = useRef(onPaths);
  useEffect(() => {
    handler.current = onPaths;
  }, [onPaths]);

  useEffect(() => {
    if (!isTauri()) return;
    let stopped = false;
    let unlisten: (() => void) | null = null;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "drop") {
            setOver(false);
            if (payload.paths.length > 0) handler.current(payload.paths);
          } else if (payload.type === "leave") {
            setOver(false);
          } else {
            setOver(true);
          }
        }),
      )
      .then((stop) => {
        if (stopped) stop();
        else unlisten = stop;
      })
      .catch(() => undefined);
    return () => {
      stopped = true;
      unlisten?.();
    };
  }, []);

  return over;
}
