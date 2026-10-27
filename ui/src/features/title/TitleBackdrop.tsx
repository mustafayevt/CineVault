import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import type { CSSProperties, ReactNode } from "react";
import { createPortal } from "react-dom";

import { ROUTE_CONTENT_ATTRIBUTE } from "../shell/useRouteFocus";
import { BACKDROP_SLOT_ID } from "./backdropSlot";

const subscribeNever = () => () => {};
const findSlot = () => document.getElementById(BACKDROP_SLOT_ID);

interface TitleBackdropProps {
  /** Height of the band below the title bar, in px. */
  height: number;
  children: ReactNode;
}

/**
 * The hero backdrop band of a title page. In the app it is drawn into the shell's backdrop layer,
 * so it bleeds from the window's top-left under the sidebar panel and the title bar and across
 * the scrollbar gutter (never a square edge against the panel), and follows the
 * page scroll. There the sharp artwork starts at the page column and a blurred copy fills the edge
 * under the panel. Without a shell around it (a lone header in a test) it stays inside the header.
 */
export function TitleBackdrop({ height, children }: TitleBackdropProps) {
  const band = useRef<HTMLDivElement>(null);
  // The layer is in the shell's first commit, after this page's first render: React reads the
  // snapshot again once mounted and draws the band into the layer then.
  const slot = useSyncExternalStore(subscribeNever, findSlot, () => null);

  useLayoutEffect(() => {
    const element = band.current;
    const scroller = document.querySelector<HTMLElement>(`[${ROUTE_CONTENT_ATTRIBUTE}]`);
    if (element === null || slot === null || scroller === null) return;
    const follow = () => {
      element.style.transform = `translateY(${-scroller.scrollTop}px)`;
    };
    // Where the page column starts in the layer: the sharp artwork begins there.
    const measure = () => {
      const left = scroller.getBoundingClientRect().left - slot.getBoundingClientRect().left;
      element.style.setProperty("--content-left", `${left}px`);
    };
    follow();
    measure();
    const resize = new ResizeObserver(measure);
    resize.observe(scroller);
    scroller.addEventListener("scroll", follow, { passive: true });
    return () => {
      resize.disconnect();
      scroller.removeEventListener("scroll", follow);
    };
  }, [slot]);

  const style = { "--backdrop-h": `${height}px` } as CSSProperties;
  const bandEl = (
    <div
      ref={band}
      aria-hidden="true"
      className="cv-title-backdrop"
      data-in-shell={slot === null ? undefined : ""}
      style={style}
    >
      {slot === null ? null : (
        <div className="cv-title-backdrop-layer cv-title-backdrop-soft">{children}</div>
      )}
      <div className="cv-title-backdrop-layer cv-title-backdrop-sharp">{children}</div>
    </div>
  );
  return slot === null ? bandEl : createPortal(bandEl, slot);
}
