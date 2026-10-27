import { Popover as RadixPopover } from "radix-ui";
import type { CSSProperties, ReactElement, ReactNode } from "react";

import { cx } from "../lib/cx";
import { floatingSurface, useFloatingContainer } from "./floating";
import { SurfaceContext } from "./surface";

interface PopoverProps {
  /** The control that opens the popover; it gets `aria-expanded` and `aria-controls`. */
  trigger: ReactElement;
  children: ReactNode;
  /** Names the popover's dialog for assistive technology. */
  "aria-label"?: string;
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  align?: "start" | "center" | "end";
  side?: "top" | "right" | "bottom" | "left";
  /** Padding 12 by default; pass `p-0` (or another) to lay out the content edge to edge. */
  className?: string;
  style?: CSSProperties;
  /** Where focus goes when it opens; by default the first focusable control inside. */
  onOpenAutoFocus?: (event: Event) => void;
}

/**
 * A small floating panel anchored to its trigger (a filter, a column chooser, a quick picker). It
 * fades in and grows from the trigger (`.cv-pop`, origin from the anchor), Esc and a click
 * outside close it, and focus returns to the trigger. Fields inside take the well colour.
 */
export function Popover({
  trigger,
  children,
  "aria-label": ariaLabel,
  open,
  defaultOpen,
  onOpenChange,
  align = "start",
  side = "bottom",
  className,
  style,
  onOpenAutoFocus,
}: PopoverProps) {
  const container = useFloatingContainer();
  return (
    <RadixPopover.Root open={open} defaultOpen={defaultOpen} onOpenChange={onOpenChange}>
      <RadixPopover.Trigger asChild>{trigger}</RadixPopover.Trigger>
      <RadixPopover.Portal container={container}>
        <RadixPopover.Content
          aria-label={ariaLabel}
          align={align}
          side={side}
          sideOffset={4}
          collisionPadding={8}
          onOpenAutoFocus={onOpenAutoFocus}
          className={cx(
            floatingSurface,
            "max-h-(--radix-popover-content-available-height) overflow-y-auto rounded-(--r-10) p-3",
            className,
          )}
          style={style}
        >
          <SurfaceContext value="floating">{children}</SurfaceContext>
        </RadixPopover.Content>
      </RadixPopover.Portal>
    </RadixPopover.Root>
  );
}
