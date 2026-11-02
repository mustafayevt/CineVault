import { useEffect, useRef, useState } from "react";

/**
 * The width of the work pane (the shell's `main`, the `pane` container) in CSS pixels as it
 * resizes, for a layout that has to change in script (a table's columns) where a container query
 * cannot reach. The ref goes on any element inside the pane. `null` until it is measured, and where
 * there is no layout (tests), so callers keep their full layout.
 */
export function usePaneWidth<T extends HTMLElement = HTMLDivElement>() {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState<number | null>(null);
  useEffect(() => {
    const element = ref.current;
    if (element === null || typeof ResizeObserver === "undefined") return;
    const pane = element.closest("main") ?? element;
    // A width of 0 is an element not laid out yet (or a test stub): keep the full layout.
    const measure = () => setWidth(pane.clientWidth > 0 ? pane.clientWidth : null);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(pane);
    return () => observer.disconnect();
  }, []);
  return { ref, width };
}
