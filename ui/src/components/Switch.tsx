import type { ButtonHTMLAttributes } from "react";

import { cx } from "../lib/cx";

interface SwitchProps extends Omit<
  ButtonHTMLAttributes<HTMLButtonElement>,
  "onChange" | "role" | "children"
> {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

/**
 * An on/off switch: a 32 x 18 pill, gold when on with a dark thumb that glides across, a neutral
 * track with a muted thumb when off. Space and Enter toggle it. Name it with `aria-labelledby` or
 * `aria-label`.
 */
export function Switch({ checked, onCheckedChange, className, disabled, ...rest }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cx("inline-flex shrink-0 items-center gap-2 rounded-(--r-pill) p-0", className)}
      {...rest}
    >
      <span
        aria-hidden="true"
        className={cx(
          "cv-hoverable relative block h-[18px] w-8 rounded-(--r-pill)",
          checked ? "bg-gold" : "bg-track",
          disabled && (checked ? "bg-text-disabled" : "bg-track"),
        )}
      >
        <span
          className={cx(
            "absolute top-0.5 left-0.5 size-3.5 rounded-(--r-pill) transition-transform",
            "duration-(--dur-hover) ease-(--ease-out) motion-reduce:transition-none",
            checked ? "translate-x-3.5 bg-on-gold" : "translate-x-0 bg-text-muted",
          )}
        />
      </span>
    </button>
  );
}
