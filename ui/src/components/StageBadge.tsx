import { Check, TriangleAlert } from "lucide-react";

import { cx } from "../lib/cx";
import { stageWord } from "./stageWord";
import { Tooltip } from "./Tooltip";

/** A title's stage, or `attention` for a title that needs the user (not matched, needs review). */
export type StageBadgeKind = "new" | "scraped" | "renamed" | "attention";

interface StageBadgeProps {
  stage: StageBadgeKind;
  /** The accessible name and tooltip; defaults to the stage word ("Scraped"). Say "Not matched" for a warning. */
  label?: string;
  className?: string;
}

/** The glyph alone: a green check, an orange warning, or a hollow muted dot. */
export function StageGlyph({ stage }: { stage: StageBadgeKind }) {
  return (
    <>
      {stage === "new" && (
        <span
          aria-hidden="true"
          className="box-border size-[7px] rounded-(--r-pill) border-[1.5px] border-solid border-text-muted"
        />
      )}
      {(stage === "scraped" || stage === "renamed") && (
        <Check
          aria-hidden="true"
          focusable="false"
          size={16}
          strokeWidth={2.4}
          className="text-ok"
        />
      )}
      {stage === "attention" && (
        <TriangleAlert
          aria-hidden="true"
          focusable="false"
          size={16}
          strokeWidth={1.75}
          className="text-warn"
        />
      )}
    </>
  );
}

/**
 * The stage glyph of a row or poster corner: a green check for Scraped and Renamed, an orange
 * warning for what needs attention, a hollow muted dot for New. Always named (role img + label),
 * so state is never colour alone. The name is also its tooltip (the app's, never a native one),
 * and the glyph is not a control, so the pointer stays an arrow over it.
 */
export function StageBadge({ stage, label, className }: StageBadgeProps) {
  const name = label ?? stageWord(stage);
  return (
    <Tooltip content={name}>
      <span
        role="img"
        aria-label={name}
        data-stage={stage}
        className={cx(
          "inline-flex size-4 shrink-0 cursor-default items-center justify-center",
          className,
        )}
      >
        <StageGlyph stage={stage} />
      </span>
    </Tooltip>
  );
}
