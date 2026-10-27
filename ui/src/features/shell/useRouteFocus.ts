import { useRouter } from "@tanstack/react-router";
import { useEffect } from "react";

/** Marks where the routed page is drawn (`data-route-content` in the shell); its first heading
 * takes the focus after a navigation. */
export const ROUTE_CONTENT_ATTRIBUTE = "data-route-content";

/** How long a page may take to draw its heading (a detail page loads first), in ms. */
const WAIT_MS = 3000;
const STEP_MS = 50;

function lostFocus(): boolean {
  const active = document.activeElement;
  return active === null || active === document.body || !active.isConnected;
}

/**
 * Keeps the keyboard in the page after a navigation. When the element that had the focus went
 * away with the old page (a poster opened with Enter, a view saved or deleted), the new page's
 * first heading takes the focus, so Tab goes on from the top of that page instead of from the
 * start of the window. A navigation from a control that stays (a sidebar link) keeps its focus.
 * A page that moves the focus itself (into a field) keeps that too.
 */
export function useRouteFocus(): void {
  const router = useRouter();
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stop = router.history.subscribe(() => {
      clearTimeout(timer);
      const started = Date.now();
      // The old page may still be drawn at first, and the new one may draw its heading late,
      // so this looks again for a while: whenever the focus is lost, the heading takes it.
      const attempt = () => {
        const heading = document.querySelector<HTMLElement>(
          `[${ROUTE_CONTENT_ATTRIBUTE}] h1, main h1`,
        );
        if (lostFocus() && heading !== null) {
          if (!heading.hasAttribute("tabindex")) heading.tabIndex = -1;
          heading.focus({ preventScroll: true });
        }
        if (Date.now() - started < WAIT_MS) timer = setTimeout(attempt, STEP_MS);
      };
      timer = setTimeout(attempt, 0);
    });
    return () => {
      clearTimeout(timer);
      stop();
    };
  }, [router]);
}
