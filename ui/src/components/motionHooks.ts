import { useEffect, useState, useSyncExternalStore } from "react";

/**
 * Motion helpers of the desktop design. The CSS classes in `styles/motion.css` do the
 * animating; these hooks only decide when something shows and whether motion is reduced.
 */

const REDUCED_QUERY = "(prefers-reduced-motion: reduce)";

/**
 * Whether motion is reduced right now: the Motion setting writes `data-motion` on the root
 * (`reduced` or `full`); without it the system preference decides, as in motion.css.
 */
function prefersReducedMotion(): boolean {
  if (typeof document === "undefined") return false;
  const setting = document.documentElement.dataset.motion;
  if (setting === "reduced") return true;
  if (setting === "full") return false;
  return typeof window.matchMedia === "function" && window.matchMedia(REDUCED_QUERY).matches;
}

function subscribeMotion(onChange: () => void): () => void {
  const observer = new MutationObserver(onChange);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-motion"],
  });
  const media = typeof window.matchMedia === "function" ? window.matchMedia(REDUCED_QUERY) : null;
  media?.addEventListener?.("change", onChange);
  return () => {
    observer.disconnect();
    media?.removeEventListener?.("change", onChange);
  };
}

/** `prefersReducedMotion()` as state: re-renders when the setting or the system preference changes. */
export function useReducedMotion(): boolean {
  return useSyncExternalStore(subscribeMotion, prefersReducedMotion, () => false);
}

/**
 * Turns true only after `ms` milliseconds of `active` being true, and false again at once when it
 * stops. Skeletons use `useLateFlag(150)` so fast data never flashes a placeholder.
 */
export function useLateFlag(ms: number, active = true): boolean {
  const [late, setLate] = useState(false);
  useEffect(() => {
    if (!active) return;
    const timer = window.setTimeout(() => setLate(true), ms);
    return () => {
      window.clearTimeout(timer);
      setLate(false);
    };
  }, [ms, active]);
  return active && late;
}
