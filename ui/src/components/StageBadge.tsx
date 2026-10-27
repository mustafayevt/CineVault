import { Check, TriangleAlert } from "lucide-react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";

/** A title's stage, or `attention` for a title that needs the user (not matched, needs review). */
export type StageBadgeKind = "new" | "scraped" | "renamed" | "attention";

interface StageBadgeProps {
  stage: StageBadgeKind;
  /** The accessible name and tooltip; defaults to the stage word ("Scraped"). Say "Not matched" for a warning. */
  label?: string;
  className?: string;
}

/**
 * The stage glyph of a row or poster corner: a green check for Scraped and Renamed, an orange
 * warning for what needs attention, a hollow muted dot for New. Always named (role img + label),
 * so state is never colour alone.
 */
export function StageBadge({ stage, label, className }: StageBadgeProps) {
  const name =
    label ??
    (stage === "new"
      ? t("stage.new")
      : stage === "scraped"
        ? t("stage.scraped")
        : stage === "renamed"
          ? t("stage.renamed")
          : t("stage.flag.needsReview"));
  return (
    <span
      role="img"
      aria-label={name}
      title={name}
      data-stage={stage}
      className={cx("inline-flex size-4 shrink-0 items-center justify-center", className)}
    >
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
    </span>
  );
}
