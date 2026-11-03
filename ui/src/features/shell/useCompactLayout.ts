import { useEffect, useSyncExternalStore } from "react";

import { isBrowser } from "../../lib/transport";

/**
 * Below this width the browser version uses the compact layout (spec `app-shell` "Phone and
 * tablet layout"): the sidebar becomes a drawer behind a menu button. The desktop window never
 * gets that narrow (its minimum width is 1100), and its zoom does not switch it either.
 */
export const COMPACT_QUERY = "(max-width: 1023.98px)";

function media(): MediaQueryList | null {
  return typeof window.matchMedia === "function" ? window.matchMedia(COMPACT_QUERY) : null;
}

function subscribe(onChange: () => void): () => void {
  const list = media();
  list?.addEventListener("change", onChange);
  return () => list?.removeEventListener("change", onChange);
}

function snapshot(): boolean {
  return isBrowser() && media()?.matches === true;
}

/** True while the browser version is narrower than a small laptop (phones, tablets in portrait). */
export function useCompactLayout(): boolean {
  return useSyncExternalStore(subscribe, snapshot, () => false);
}

/** A touch screen is the main pointer. */
const TOUCH_QUERY = "(pointer: coarse)";

/**
 * Marks the root `data-touch` while the browser version runs on a touch screen, which raises the
 * control height tokens to touch sizes (`tokens.css`). The desktop window is left as it is, also on
 * a touch laptop.
 */
export function useTouchTargets(): void {
  useEffect(() => {
    if (!isBrowser() || typeof window.matchMedia !== "function") return;
    const list = window.matchMedia(TOUCH_QUERY);
    const root = document.documentElement;
    const apply = () => {
      if (list.matches) root.dataset.touch = "true";
      else delete root.dataset.touch;
    };
    apply();
    list.addEventListener("change", apply);
    return () => {
      list.removeEventListener("change", apply);
      delete root.dataset.touch;
    };
  }, []);
}
