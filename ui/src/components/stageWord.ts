import { t } from "../i18n/catalog";
import type { StageBadgeKind } from "./StageBadge";

/** The word of a stage: "New", "Scraped", "Renamed", or "Needs review" for a title that needs attention. */
export function stageWord(stage: StageBadgeKind): string {
  return stage === "new"
    ? t("stage.new")
    : stage === "scraped"
      ? t("stage.scraped")
      : stage === "renamed"
        ? t("stage.renamed")
        : t("stage.flag.needsReview");
}
