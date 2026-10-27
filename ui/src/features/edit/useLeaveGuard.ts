import { useBlocker } from "@tanstack/react-router";
import { useCallback, useEffect, useRef } from "react";

/** What the unsaved-changes guard is doing about a navigation. */
export interface LeaveGuard {
  /** A navigation is waiting for the user to choose. */
  blocked: boolean;
  /** Lets the navigation happen. */
  proceed: () => void;
  /** Cancels the navigation and stays on the page. */
  stay: () => void;
}

/**
 * Holds a navigation away from the page while `dirty` is true, so the page can ask what to do
 * with the unsaved changes inline (the dirty bar). Closing the window is guarded by the browser's
 * own prompt.
 */
export function useLeaveGuard(dirty: boolean): LeaveGuard {
  const dirtyNow = useRef(dirty);
  useEffect(() => {
    dirtyNow.current = dirty;
  }, [dirty]);
  const isDirtyNow = useCallback(() => dirtyNow.current, []);
  // Moving between the tabs of the page, or opening a workspace on it, only changes the search of
  // the same page: the edit session stays, so nothing is left behind and nothing is asked.
  const leavesPage = useCallback(
    ({ current, next }: { current: { pathname: string }; next: { pathname: string } }) =>
      dirtyNow.current && current.pathname !== next.pathname,
    [],
  );
  const blocker = useBlocker({
    shouldBlockFn: leavesPage,
    enableBeforeUnload: isDirtyNow,
    withResolver: true,
  });
  return {
    blocked: blocker.status === "blocked",
    proceed: () => blocker.proceed?.(),
    stay: () => blocker.reset?.(),
  };
}
