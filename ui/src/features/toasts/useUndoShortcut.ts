import { useEffect } from "react";

import type { Platform } from "../shell/platform";
import { isTextField } from "../delete/deleteKeys";
import { useToastStore } from "./toastStore";

/** The text shown on the Undo button for its key: `Ctrl Z`, or `⌘Z` on macOS. */
export function undoShortcutLabel(platform: Platform): string {
  return platform === "macos" ? "⌘Z" : "Ctrl Z";
}

/**
 * Undoes the newest action that can still be undone on Ctrl Z (⌘Z on macOS), whether its toast
 * still shows or has timed out. While a text field has focus the key keeps its native text-undo
 * meaning, and with nothing to undo the key is left alone.
 */
export function useUndoShortcut(platform: Platform): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.shiftKey || event.altKey) return;
      if (event.key.toLowerCase() !== "z") return;
      const modifier = platform === "macos" ? event.metaKey && !event.ctrlKey : event.ctrlKey;
      if (!modifier || isTextField(event.target)) return;
      const { history, undoLatest } = useToastStore.getState();
      if (history.length === 0) return;
      event.preventDefault();
      void undoLatest();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [platform]);
}
