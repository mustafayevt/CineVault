import type { CSSProperties } from "react";

import { cx } from "../lib/cx";
import "./bars.css";

export type ProgressTone = "accent" | "info";

interface ProgressBarProps {
  /** The accessible name, for example the job name. */
  label: string;
  /** 0 to 100; anything outside is clamped. */
  percent: number;
  /** Gold for local work (the default), neutral for work that waits on the network. */
  tone?: ProgressTone;
  /** Track thickness: `sm` is 3px (the thin bar), `md` is 6px. */
  size?: "sm" | "md";
  /** Work whose length is not known: a short fill sweeps across; still under reduced motion. */
  indeterminate?: boolean;
  className?: string;
}

/**
 * A progress bar: a `--track` groove with a gold fill. The fill is always full width and scaled
 * from the left with `transform: scaleX` (`.cv-bar`, the `--progress` variable), so a new value
 * glides in over `--dur-bar` and never animates a layout property.
 */
export function ProgressBar({
  label,
  percent,
  tone = "accent",
  size = "sm",
  indeterminate = false,
  className,
}: ProgressBarProps) {
  const value = Math.min(100, Math.max(0, Number.isFinite(percent) ? percent : 0));
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(value)}
      className={cx(
        "w-full overflow-hidden rounded-(--r-pill) bg-track",
        size === "sm" ? "h-[3px]" : "h-1.5",
        className,
      )}
    >
      <div
        data-testid="progress-fill"
        className={cx(
          "cv-bar rounded-(--r-pill)",
          indeterminate && "cv-bar-indeterminate",
          tone === "info" ? "bg-bar-neutral" : "bg-gold",
        )}
        style={{ "--progress": value / 100 } as CSSProperties}
      />
    </div>
  );
}
