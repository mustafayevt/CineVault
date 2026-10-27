import { useCallback, useRef } from "react";
import { create } from "zustand";

/**
 * Bars that sit along the bottom of the window (the workspace action bar, the bulk-selection bar)
 * report how much of the window bottom they cover, so floating things such as the toast stack can
 * keep clear of them. Each bar reports under its own id and removes itself when it unmounts.
 */
interface BottomInsetState {
  /** Pixels covered from the bottom edge of the window, by bar id. */
  insets: Readonly<Record<string, number>>;
  set: (id: string, inset: number) => void;
  clear: (id: string) => void;
}

export const useBottomInsetStore = create<BottomInsetState>((set) => ({
  insets: {},
  set: (id, inset) =>
    set((state) =>
      state.insets[id] === inset ? state : { insets: { ...state.insets, [id]: inset } },
    ),
  clear: (id) =>
    set((state) => {
      if (!(id in state.insets)) return state;
      return {
        insets: Object.fromEntries(Object.entries(state.insets).filter(([key]) => key !== id)),
      };
    }),
}));

/** The tallest bar along the bottom, in pixels; 0 when there is none. */
export function selectBottomInset(state: BottomInsetState): number {
  return Math.max(0, ...Object.values(state.insets));
}

/**
 * A ref callback for a bottom bar: it reports the distance from the bar's top edge to the bottom
 * of the window under `id`, now and whenever the bar changes size, and clears it on unmount.
 */
export function useReportBottomInset(id: string): (element: HTMLElement | null) => void {
  const cleanup = useRef<(() => void) | null>(null);
  return useCallback(
    (element: HTMLElement | null) => {
      cleanup.current?.();
      cleanup.current = null;
      if (element === null) return;
      const { set, clear } = useBottomInsetStore.getState();
      const report = () =>
        set(id, Math.max(0, Math.round(window.innerHeight - element.getBoundingClientRect().top)));
      report();
      const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(report);
      observer?.observe(element);
      window.addEventListener("resize", report);
      cleanup.current = () => {
        observer?.disconnect();
        window.removeEventListener("resize", report);
        clear(id);
      };
    },
    [id],
  );
}
