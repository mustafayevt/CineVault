import { Tooltip as RadixTooltip } from "radix-ui";
import { createContext, useContext, useState } from "react";
import type { ReactElement, ReactNode } from "react";

import { cx } from "../lib/cx";
import { floatingSurface, useFloatingContainer } from "./floating";

/** The rest time before a tooltip shows (spec desktop-interaction "Tooltips after a short delay"). */
const TOOLTIP_DELAY_MS = 500;

const ProvidedContext = createContext(false);

/**
 * One provider for the app: moving from one tooltip to the next shows it without the delay again.
 * A Tooltip outside it brings its own, so components and tests work without one.
 */
export function TooltipProvider({ children }: { children: ReactNode }) {
  return (
    <RadixTooltip.Provider delayDuration={TOOLTIP_DELAY_MS} skipDelayDuration={300}>
      <ProvidedContext value={true}>{children}</ProvidedContext>
    </RadixTooltip.Provider>
  );
}

interface TooltipProps {
  /** The text shown. Nothing is shown when it is empty. */
  content: ReactNode;
  /** The element the tooltip belongs to; it must take a ref and pass on pointer and focus events. */
  children: ReactElement;
  side?: "top" | "right" | "bottom" | "left";
  align?: "start" | "center" | "end";
  /** Show only while the element's text is cut off by an ellipsis (a truncated title cell). */
  whenTruncated?: boolean;
  className?: string;
}

/** Whether the element, or its first child (the text span of a cell), is cut off. */
function isTruncated(element: Element | null): boolean {
  if (!(element instanceof HTMLElement)) return false;
  if (element.scrollWidth > element.clientWidth) return true;
  const inner = element.firstElementChild;
  return inner instanceof HTMLElement && inner.scrollWidth > inner.clientWidth;
}

/**
 * Whether the last thing the user did was move focus with the keyboard (Tab, arrows, Home, End).
 * Focus that a script hands over (a dialog focusing its first control, a menu handing focus back)
 * opens no tooltip; only focus the user moved there does.
 */
let keyboardMovedFocus = false;
const NAVIGATION_KEYS = new Set([
  "Tab",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Home",
  "End",
]);
if (typeof document !== "undefined") {
  document.addEventListener(
    "keydown",
    (event) => {
      keyboardMovedFocus = NAVIGATION_KEYS.has(event.key);
    },
    true,
  );
  document.addEventListener(
    "pointerdown",
    () => {
      keyboardMovedFocus = false;
    },
    true,
  );
}

function hovered(element: Element): boolean {
  try {
    return element.matches(":hover");
  } catch {
    return false;
  }
}

/** An open request from focus counts only when the keyboard moved focus there. */
function wantedOpen(element: Element | null): boolean {
  if (element === null || document.activeElement !== element) return true;
  return keyboardMovedFocus || hovered(element);
}

/**
 * A short label on the floating surface (radius 6, 12px). It shows after the pointer rests for
 * 500 ms, at once when the user moves focus to it with the keyboard, and hides when the pointer leaves, on Esc,
 * on a click and on scroll. Fades in and grows from 0.98 at the element (`.cv-pop`).
 */
export function Tooltip({
  content,
  children,
  side = "top",
  align = "center",
  whenTruncated,
  className,
}: TooltipProps) {
  const provided = useContext(ProvidedContext);
  const container = useFloatingContainer();
  const [open, setOpen] = useState(false);
  const [trigger, setTrigger] = useState<Element | null>(null);
  const empty = content === null || content === undefined || content === "";

  const root = (
    <RadixTooltip.Root
      open={open && !empty}
      onOpenChange={(next) =>
        setOpen(next && wantedOpen(trigger) && (whenTruncated !== true || isTruncated(trigger)))
      }
    >
      <RadixTooltip.Trigger asChild ref={setTrigger}>
        {children}
      </RadixTooltip.Trigger>
      <RadixTooltip.Portal container={container}>
        <RadixTooltip.Content
          side={side}
          align={align}
          sideOffset={6}
          collisionPadding={8}
          className={cx(
            floatingSurface,
            "max-w-80 rounded-(--r-6) px-2 py-1 text-(length:--fs-12) leading-snug select-none",
            className,
          )}
        >
          {content}
        </RadixTooltip.Content>
      </RadixTooltip.Portal>
    </RadixTooltip.Root>
  );

  return provided ? root : <TooltipProvider>{root}</TooltipProvider>;
}
