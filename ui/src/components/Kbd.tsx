import type { ReactNode } from "react";

import { cx } from "../lib/cx";

interface KbdProps {
  className?: string;
  /** The key or chord, for example `Del`, `Shift Del` or `Ctrl K`. */
  children: ReactNode;
}

/**
 * A key cap: 10.5px, 1px + 5px padding, radius 4, a `--line-kbd` edge, muted text. Shortcut hints
 * are words, so it keeps the interface face (base.css).
 */
export function Kbd({ className, children }: KbdProps) {
  return (
    <kbd
      className={cx(
        "inline-flex items-center rounded-(--r-4) border border-solid border-line-kbd px-[5px] py-px",
        "text-[10.5px] leading-[1.3] font-(weight:--fw-500) whitespace-nowrap text-text-muted",
        className,
      )}
    >
      {children}
    </kbd>
  );
}
