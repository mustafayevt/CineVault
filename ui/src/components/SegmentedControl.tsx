import type { LucideIcon } from "lucide-react";
import { useRef } from "react";
import type { KeyboardEvent } from "react";

import { cx } from "../lib/cx";
import { Icon } from "./Icon";

export interface SegmentOption<T extends string> {
  value: T;
  /** Visible text. Leave out for an icon-only segment, which then needs `ariaLabel`. */
  label?: string;
  icon?: LucideIcon;
  /** Accessible name for icon-only segments. */
  ariaLabel?: string;
  /** A count shown after the label in muted tabular figures ("New 21"). */
  count?: number | string;
}

interface SegmentedControlProps<T extends string> {
  options: readonly SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Names the group, for example "Library lens". */
  "aria-label": string;
  className?: string;
}

/**
 * Switches between 2-5 mutually exclusive modes: a card-coloured track (2px padding, radius 6)
 * with 24px segments (radius 4, 12.5/500); the active one is `--surface-active` and 600. The
 * accent is never used here. One tab stop: arrows, Home and End move to the next segment and
 * choose it, like a radio group.
 */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  className,
  "aria-label": ariaLabel,
}: SegmentedControlProps<T>) {
  const track = useRef<HTMLDivElement>(null);
  const activeIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, current: number) {
    const last = options.length - 1;
    let next: number;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        next = current === last ? 0 : current + 1;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        next = current === 0 ? last : current - 1;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = last;
        break;
      default:
        return;
    }
    event.preventDefault();
    const option = options[next];
    if (option === undefined) return;
    onChange(option.value);
    track.current?.querySelector<HTMLButtonElement>(`[data-index="${next}"]`)?.focus();
  }

  return (
    <div
      ref={track}
      role="group"
      aria-label={ariaLabel}
      className={cx(
        "inline-flex gap-px rounded-(--r-6) border border-solid border-line-section bg-surface-card p-0.5",
        className,
      )}
    >
      {options.map((option, index) => {
        const active = option.value === value;
        const iconOnly = !option.label;
        return (
          <button
            key={option.value}
            type="button"
            data-index={index}
            aria-pressed={active}
            aria-label={iconOnly ? option.ariaLabel : undefined}
            tabIndex={index === activeIndex ? 0 : -1}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => onKeyDown(event, index)}
            className={cx(
              "cv-hoverable inline-flex h-(--h-seg) items-center justify-center gap-1.5 rounded-(--r-4)",
              "border-0 text-[12.5px] leading-none whitespace-nowrap",
              iconOnly ? "w-8 px-0" : "px-[11px]",
              active
                ? "bg-surface-active font-(weight:--fw-600) text-text"
                : "bg-transparent font-(weight:--fw-500) text-text-2 hover:bg-surface-raised hover:text-text active:bg-surface-pressed",
            )}
          >
            {option.icon && <Icon icon={option.icon} />}
            {option.label}
            {option.count !== undefined && (
              <span
                className={cx(
                  "text-(length:--fs-11) font-(weight:--fw-500) tabular-nums",
                  "text-text-muted",
                )}
              >
                {option.count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
