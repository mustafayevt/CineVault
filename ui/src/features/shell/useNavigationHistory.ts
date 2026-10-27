import { useRouter } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";

export interface NavigationHistory {
  canGoBack: boolean;
  canGoForward: boolean;
  back: () => void;
  forward: () => void;
}

function positionOf(state: unknown): number {
  const index = (state as { __TSR_index?: unknown } | undefined)?.__TSR_index;
  return typeof index === "number" ? index : 0;
}

/** Back/forward availability for the title bar, derived from the router's history position. */
export function useNavigationHistory(): NavigationHistory {
  const router = useRouter();
  const history = router.history;
  const furthest = useRef(0);
  const [position, setPosition] = useState(() => positionOf(history.location.state));
  const [limit, setLimit] = useState(position);

  useEffect(() => {
    const sync = (action?: string) => {
      const index = positionOf(history.location.state);
      // A push drops everything that was ahead; going back or forward keeps it.
      furthest.current = action === "PUSH" ? index : Math.max(furthest.current, index);
      setPosition(index);
      setLimit(furthest.current);
    };
    sync();
    return history.subscribe(({ action }) => sync(action.type));
  }, [history]);

  return {
    canGoBack: position > 0,
    canGoForward: position < limit,
    back: () => history.back(),
    forward: () => history.forward(),
  };
}
