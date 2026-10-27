import type { ReactNode } from "react";

import { cx } from "../../lib/cx";

/** A pressed-or-not chip for a workspace's filters (language, destination); never a dropdown. */
export function FilterChip({
  pressed,
  onClick,
  children,
  label,
  disabled = false,
}: {
  pressed: boolean;
  onClick: () => void;
  children: ReactNode;
  /** The accessible name, when the visible content is more than the name. */
  label?: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={cx(
        "inline-flex h-7 items-center gap-1.5 rounded-(--r-6) px-2.5 text-[13px] whitespace-nowrap",
        "transition-colors duration-(--dur-hover) ease-(--ease-out) disabled:opacity-50",
        pressed
          ? "bg-surface-active text-text shadow-[0_0_0_1px_var(--line-edge)]"
          : "bg-surface-card text-text-2 enabled:hover:text-text",
      )}
    >
      {children}
    </button>
  );
}
