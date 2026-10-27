import type { ReactNode } from "react";

import { cx } from "../lib/cx";

interface SectionLabelProps {
  children: ReactNode;
  /** A quiet action at the right end of the label row ("Edit", "All streams"). */
  action?: ReactNode;
  /** Heading level for the label; defaults to h2. */
  as?: "h2" | "h3" | "h4" | "p";
  id?: string;
  className?: string;
}

/**
 * The one section label: 11px, 600, uppercase with 0.06em tracking, in the muted text colour,
 * with an optional quiet action at the right. Used above a card, a group of rows or a sidebar
 * section (Drives); the text is written in sentence case and the style does the capitals.
 */
export function SectionLabel({
  children,
  action,
  as: Tag = "h2",
  id,
  className,
}: SectionLabelProps) {
  return (
    <div className={cx("mb-3 flex min-h-5 items-center gap-2", className)}>
      <Tag
        id={id}
        className="m-0 text-(length:--fs-11) leading-[1.3] font-(weight:--fw-600) tracking-[0.06em] text-text-muted uppercase"
      >
        {children}
      </Tag>
      {action === undefined ? null : (
        <span className="ml-auto text-(length:--fs-12) text-text-muted">{action}</span>
      )}
    </div>
  );
}
