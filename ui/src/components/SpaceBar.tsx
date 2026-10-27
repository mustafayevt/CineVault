import type { CSSProperties } from "react";

import { cx } from "../lib/cx";

export type SpaceBarState = "normal" | "low" | "offline";

interface SpaceBarProps {
  /** The share of the drive that is used, 0 to 1 (clamped). */
  used: number;
  /** The accessible name with the numbers, for example "C: System, 96% used, 41 GB free". */
  label: string;
  /** `low` is the warning colour (a drive running out of room); `offline` is the bare groove. */
  state?: SpaceBarState;
  /** `sm` is the 3px bar of the sidebar; `md` the 6px bar of a drive card. */
  size?: "sm" | "md";
  className?: string;
}

const fills: Record<SpaceBarState, string> = {
  normal: "bg-bar-neutral",
  low: "bg-warn-bar",
  offline: "bg-track",
};

/**
 * The used-space bar of a drive: a 3px `--track` groove with a neutral fill (gold is for
 * progress, never for space), `--warn-bar` when the drive is low and an empty groove when it is
 * offline. Like the progress bar it scales a full-width fill (`.cv-bar`), so a change glides.
 */
export function SpaceBar({ used, label, state = "normal", size = "sm", className }: SpaceBarProps) {
  const share =
    state === "offline" ? 0 : Math.min(1, Math.max(0, Number.isFinite(used) ? used : 0));
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(share * 100)}
      data-state={state}
      className={cx(
        "w-full overflow-hidden rounded-(--r-pill) bg-track",
        size === "md" ? "h-1.5" : "h-[3px]",
        className,
      )}
    >
      <div
        data-testid="space-fill"
        className={cx("cv-bar rounded-(--r-pill)", fills[state])}
        style={{ "--progress": share } as CSSProperties}
      />
    </div>
  );
}
