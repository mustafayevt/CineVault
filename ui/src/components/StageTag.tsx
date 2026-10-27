import { cx } from "../lib/cx";
import { StageGlyph } from "./StageBadge";
import { stageWord } from "./stageWord";
import type { StageBadgeKind } from "./StageBadge";
import { Tooltip } from "./Tooltip";

interface StageTagProps {
  stage: StageBadgeKind;
  /** The word on the tag; the stage word ("Scraped") by default, "Not matched" for a problem. */
  label?: string;
  /** The app tooltip; the word by default. A problem says what is wrong here. */
  tip?: string;
  className?: string;
}

/**
 * A title's stage as a small labelled tag in the look of the stage filter chips: the glyph and the
 * word, New, Scraped or Renamed. A title that needs attention keeps the warning colour and its
 * problem word. It is information, not a control: the pointer stays an arrow, and the tooltip is
 * the app's own.
 */
export function StageTag({ stage, label, tip, className }: StageTagProps) {
  const word = label ?? stageWord(stage);
  return (
    <Tooltip content={tip ?? word}>
      <span
        data-stage={stage}
        className={cx(
          "inline-flex h-5 max-w-full min-w-0 cursor-default items-center gap-1 rounded-(--r-pill) px-1.5 text-(length:--fs-12)",
          stage === "attention" ? "bg-warn-tint text-warn" : "bg-surface-card text-text-2",
          className,
        )}
      >
        <span className="inline-flex size-4 shrink-0 items-center justify-center">
          <StageGlyph stage={stage} />
        </span>
        <span className="truncate">{word}</span>
      </span>
    </Tooltip>
  );
}
