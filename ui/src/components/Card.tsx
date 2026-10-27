import type { ComponentProps } from "react";

import { cx } from "../lib/cx";

export { SectionLabel } from "./SectionLabel";

type CardProps = ComponentProps<"section"> & {
  /** Tighter padding for a card in a narrow column or a dense list. */
  compact?: boolean;
};

/**
 * A block on the card step of the surface ladder: card colour, a hairline edge, radius 10 and
 * padding 14/16 (compact 10/12). Flat: no shadow, because only what floats has one. Use for the
 * Overview cards, settings groups and the detail page's panels.
 */
export function Card({ compact = false, className, children, ...rest }: CardProps) {
  return (
    <section
      className={cx(
        "min-w-0 rounded-(--r-10) border border-solid border-line-section bg-surface-card",
        compact ? "px-3 py-2.5" : "px-4 py-3.5",
        className,
      )}
      {...rest}
    >
      {children}
    </section>
  );
}
