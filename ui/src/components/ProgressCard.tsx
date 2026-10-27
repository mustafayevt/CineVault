import { cx } from "../lib/cx";
import "./bars.css";
import { ProgressBar } from "./ProgressBar";
import { Tooltip } from "./Tooltip";

interface ProgressCardProps {
  /** The bold line: "Scraping 3 of 25". Also the accessible name. */
  title: string;
  /** A muted line under the bold one, such as "+1 more" while other work runs. */
  more?: string;
  /** 0 to 100. */
  percent: number;
  /** What it is working on right now ("Heat (1995): writing artwork"); shown as a tooltip. */
  detail?: string;
  /** The sidebar is collapsed: a ring with the percentage instead of the card. */
  collapsed?: boolean;
  className?: string;
}

const RING = 32;
const STROKE = 3;
const RADIUS = (RING - STROKE) / 2;

/**
 * The sidebar's progress card: the well colour at radius 6 (it sits 8px inside
 * the 14px panel), a bold line, the percentage muted at the right and a thin gold bar. Collapsed,
 * it is a 32px ring with the percentage inside. The current item is a tooltip, not more text.
 */
export function ProgressCard({
  title,
  more,
  percent,
  detail,
  collapsed = false,
  className,
}: ProgressCardProps) {
  const value = Math.min(100, Math.max(0, Number.isFinite(percent) ? percent : 0));
  const rounded = Math.round(value);
  const body = collapsed ? (
    <div
      role="progressbar"
      aria-label={title}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={rounded}
      className={cx(
        "relative flex size-8 items-center justify-center rounded-(--r-pill)",
        className,
      )}
    >
      <svg aria-hidden="true" width={RING} height={RING} viewBox={`0 0 ${RING} ${RING}`}>
        <circle
          cx={RING / 2}
          cy={RING / 2}
          r={RADIUS}
          fill="none"
          strokeWidth={STROKE}
          className="stroke-track"
        />
        <circle
          data-testid="progress-ring"
          cx={RING / 2}
          cy={RING / 2}
          r={RADIUS}
          fill="none"
          strokeWidth={STROKE}
          strokeLinecap="round"
          pathLength={100}
          strokeDasharray={100}
          strokeDashoffset={100 - value}
          transform={`rotate(-90 ${RING / 2} ${RING / 2})`}
          className="cv-ring-arc stroke-gold"
        />
      </svg>
      <span
        aria-hidden="true"
        className="absolute text-(length:--fs-11) font-(weight:--fw-600) text-text tabular-nums"
      >
        {rounded}
      </span>
    </div>
  ) : (
    <div
      data-testid="progress-card"
      className={cx(
        "flex flex-col gap-2 rounded-(--r-6) bg-surface-well px-3 pt-2.5 pb-3",
        className,
      )}
    >
      <div className="flex items-baseline justify-between gap-2 text-(length:--fs-12) whitespace-nowrap">
        <span className="min-w-0 truncate font-(weight:--fw-600) text-text">{title}</span>
        <span className="text-text-muted tabular-nums">{rounded}%</span>
      </div>
      {more === undefined ? null : (
        <span className="-mt-1 truncate text-(length:--fs-12) text-text-muted">{more}</span>
      )}
      <ProgressBar label={title} percent={value} />
    </div>
  );
  return detail === undefined ? (
    body
  ) : (
    <Tooltip content={detail} side={collapsed ? "right" : "top"}>
      {body}
    </Tooltip>
  );
}
