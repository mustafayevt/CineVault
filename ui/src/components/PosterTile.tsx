import { Check } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";

import { cx } from "../lib/cx";
import { PlaceholderPoster } from "./PlaceholderPoster";
import { QualityBadge } from "./QualityBadge";
import type { QualityTier } from "./QualityBadge";
import { StageBadge } from "./StageBadge";
import type { StageBadgeKind } from "./StageBadge";

type PosterTileProps = Omit<ComponentProps<"button">, "children" | "title"> & {
  /** The title as written under the poster, and the poster's accessible name. */
  title: string;
  /** The year line under the title; "????" when unknown. */
  year?: string;
  quality?: QualityTier;
  /** A short note after the badge in the warning colour: "Not matched", "Uncertain". */
  note?: string;
  stage: StageBadgeKind;
  /** The badge's name when the stage word is not enough: "Not matched" for an attention badge. */
  stageLabel?: string;
  /** Selected: a gold outline round the poster and a check in its corner. */
  selected?: boolean;
  /** The artwork (a cached image). Without it the tile draws the generated poster for the title. */
  poster?: ReactNode;
};

/**
 * A poster in the grid: a 2:3 box at radius 10 that lifts and brightens on hover (`.cv-lift`), the
 * stage badge in the top-right corner, then the title (13/600, one line) and a year, quality and
 * note line. Selected, the poster gets a 2px gold outline 3px out and a gold check at its
 * top-left. The tile is a toggle button, so Enter and Space select it; opening on double-click and
 * the right-click menu belong to the list that owns the tile.
 */
export function PosterTile({
  title,
  year,
  quality,
  note,
  stage,
  stageLabel,
  selected = false,
  poster,
  className,
  type = "button",
  ...rest
}: PosterTileProps) {
  return (
    <button
      type={type}
      aria-pressed={selected}
      data-selected={selected ? "true" : undefined}
      className={cx(
        "group flex w-full min-w-0 flex-col gap-[7px] rounded-(--r-10) border-0 bg-transparent p-0 text-left text-text",
        className,
      )}
      {...rest}
    >
      <span
        data-testid="poster-frame"
        className={cx(
          "cv-lift relative block aspect-[2/3] w-full overflow-hidden rounded-(--r-10)",
          "outline-offset-[3px]",
          selected ? "outline-2 outline-gold" : "outline-0",
        )}
      >
        {poster ?? <PlaceholderPoster title={title} />}
        <span className="absolute top-[7px] right-[7px] flex size-5 items-center justify-center rounded-(--r-pill) bg-badge-scrim backdrop-blur-[4px]">
          <StageBadge stage={stage} label={stageLabel} />
        </span>
        {selected && (
          <span
            data-testid="poster-check"
            aria-hidden="true"
            className="absolute top-[7px] left-[7px] flex size-5 items-center justify-center rounded-(--r-pill) bg-gold text-on-gold"
          >
            <Check size={12} strokeWidth={2.6} aria-hidden="true" />
          </span>
        )}
      </span>
      <span className="block truncate text-(length:--fs-13) font-(weight:--fw-600)">{title}</span>
      {(year !== undefined || quality !== undefined || note !== undefined) && (
        <span className="-mt-1.5 flex min-w-0 items-center gap-1.5 overflow-hidden text-(length:--fs-12) text-text-muted">
          {year !== undefined && <span>{year}</span>}
          {quality !== undefined && <QualityBadge quality={quality} />}
          {note !== undefined && <span className="min-w-0 truncate text-warn">{note}</span>}
        </span>
      )}
    </button>
  );
}
