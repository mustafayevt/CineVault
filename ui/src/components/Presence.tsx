import { useEffect, useState } from "react";
import type { AnimationEvent, ReactNode } from "react";

import { useReducedMotion } from "./motionHooks";

/** What Presence hands its child: spread it on the animated element. */
export interface PresenceProps {
  "data-state": "open" | "closed";
  onAnimationEnd: (event: AnimationEvent<HTMLElement>) => void;
}

interface Props {
  /** Whether the thing should be on screen. */
  present: boolean;
  /**
   * The longest the exit may take before the element goes anyway (it goes earlier when its exit
   * animation ends). Defaults to --dur-max, 250 ms.
   */
  exitMs?: number;
  /** Renders the element with `data-state` set: `open` while present, `closed` while leaving. */
  children: (props: PresenceProps) => ReactNode;
}

/**
 * Keeps an element mounted for its exit animation. While `present` turns false the
 * element stays with `data-state="closed"`, so a motion class (`.cv-slide-up`, `.cv-pop`,
 * `.cv-xfade`) plays its ease-in exit; it unmounts when that animation ends, or after `exitMs`.
 * With reduced motion the element goes at once.
 */
export function Presence({ present, exitMs = 250, children }: Props) {
  const reduced = useReducedMotion();
  const [mounted, setMounted] = useState(present);
  const [previous, setPrevious] = useState(present);
  if (present !== previous) {
    setPrevious(present);
    if (present) setMounted(true);
    else if (reduced) setMounted(false);
  }

  useEffect(() => {
    if (present || !mounted) return;
    const timer = window.setTimeout(() => setMounted(false), exitMs);
    return () => window.clearTimeout(timer);
  }, [present, mounted, exitMs]);

  if (!mounted) return null;
  return children({
    "data-state": present ? "open" : "closed",
    onAnimationEnd: (event) => {
      if (!present && event.target === event.currentTarget) setMounted(false);
    },
  });
}
