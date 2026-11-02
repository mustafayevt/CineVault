import { useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

/**
 * How much room the Library has, from the width of its own pane (not the window), so the page fits
 * whatever the shell leaves it. `phone` is a phone in portrait, `narrow` a tablet, `wide` everything
 * else. The desktop window's narrowest pane is about 884px, so the desktop is always `wide`; the
 * same edges are written as `@max-[640px]/pane` and `@max-[860px]/pane` container queries in CSS.
 */
export type PaneFit = "wide" | "narrow" | "phone";

/** At or under this pane width the Library uses its phone layout. */
export const PHONE_PANE = 640;
/** At or under this pane width the Library uses its tablet layout. */
export const NARROW_PANE = 860;

/** The fit of a pane `width` px wide; an unmeasured pane (0) counts as wide. */
export function paneFitOf(width: number): PaneFit {
  if (width <= 0) return "wide";
  if (width <= PHONE_PANE) return "phone";
  if (width <= NARROW_PANE) return "narrow";
  return "wide";
}

/** The fit of the element behind `ref`, kept up to date while it resizes. */
export function usePaneFit(ref: RefObject<HTMLElement | null>): PaneFit {
  const [fit, setFit] = useState<PaneFit>("wide");
  useLayoutEffect(() => {
    const element = ref.current;
    if (element === null) return;
    const measure = () => setFit(paneFitOf(element.clientWidth));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return fit;
}
