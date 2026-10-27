import { useRouter, useRouterState } from "@tanstack/react-router";
import { useEffect, useLayoutEffect, useRef } from "react";
import type { RefObject } from "react";

/** How long a restored position is retried while the page above it is still being drawn. */
const RESTORE_WINDOW_MS = 800;

function positionOf(state: unknown): number {
  const index = (state as { __TSR_index?: unknown } | undefined)?.__TSR_index;
  return typeof index === "number" ? index : 0;
}

/**
 * Opens every new page at its top; returns the ref for the element the pages scroll in. The
 * routed pages scroll inside the shell's main element, not the window, so the router's own scroll
 * reset does not reach them: without this, a page drawn from cached data (a season page, the next
 * season) opened at the scroll offset of the page before it. Only a change of page counts; a
 * change of the address search (a season tab, a filter) keeps the place.
 *
 * Going back or forward to a page returns to where it was left (spec `ui-workspaces` "Pickers are
 * pages": closing a picker returns to the scroll position it was opened from). The place is kept
 * per history entry; a page that is still drawing gets a moment to become tall enough.
 */
export function usePageScrollReset<T extends HTMLElement>(): RefObject<T | null> {
  const scroller = useRef<T>(null);
  const router = useRouter();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const previous = useRef(pathname);
  const places = useRef(new Map<number, number>());
  const entry = useRef(positionOf(router.history.location.state));
  const arrivedBy = useRef<string>("PUSH");

  // The place of the entry on screen, and which way the next page is reached.
  useEffect(() => {
    return router.history.subscribe(({ action, location }) => {
      arrivedBy.current = action.type;
      const index = positionOf(location.state);
      if (action.type === "PUSH") {
        // A push drops everything that was ahead.
        for (const kept of [...places.current.keys()]) {
          if (kept >= index) places.current.delete(kept);
        }
      }
      entry.current = index;
    });
  }, [router]);

  useEffect(() => {
    const element = scroller.current;
    if (element === null) return;
    const remember = () => places.current.set(entry.current, element.scrollTop);
    element.addEventListener("scroll", remember, { passive: true });
    return () => element.removeEventListener("scroll", remember);
  }, []);

  useLayoutEffect(() => {
    if (previous.current === pathname) return;
    previous.current = pathname;
    const element = scroller.current;
    if (element === null) return;
    const wanted =
      arrivedBy.current === "BACK" || arrivedBy.current === "FORWARD"
        ? (places.current.get(entry.current) ?? 0)
        : 0;
    element.scrollTop = wanted;
    element.scrollLeft = 0;
    if (wanted === 0) return;
    // The page may still be drawing: keep asking until the place can be reached.
    const started = Date.now();
    const timer = setInterval(() => {
      if (element.scrollTop >= wanted - 1 || Date.now() - started > RESTORE_WINDOW_MS) {
        clearInterval(timer);
        return;
      }
      element.scrollTop = wanted;
    }, 40);
    return () => clearInterval(timer);
  }, [pathname]);
  return scroller;
}
