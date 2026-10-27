import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";
import { formatRatingValue, ratingRank, ratingSource, ratingSourceName } from "./ratingSources";

/** One rating of a title, on the source's own scale, as the detail page and gallery show it. */
export interface RatingBadgeData {
  /** The stored source key: `imdb`, `tomatometerallcritics`, ... */
  source: string;
  /** The rating on the source's own scale: 8.5 out of 10, 92 out of 100, 4.4 out of 5. */
  value: number;
  /** The top of the scale. */
  max: number;
  /** How many people voted; 0 when unknown. */
  votes: number;
  /** When the rating was fetched, in epoch milliseconds; `null` or 0 when not known. */
  fetched_at_ms: number | null;
  /** The title's page on the source's site, when the source named it. */
  url?: string | null;
}

/** The value of a rating as the source writes it: `8.5`, `92%`, `79`, `4.4`. */
export function badgeValue(rating: RatingBadgeData, format: Formatters): string {
  return formatRatingValue(rating.source, rating.value, rating.max, format.number);
}

/**
 * What the tooltip of a rating says: `IMDb · 8.5 out of 10 · 640,000 votes · Fetched 20 Oct 2026`.
 * Percent and score sources leave the scale out, it is part of the value.
 */
export function badgeHint(rating: RatingBadgeData, format: Formatters): string {
  const info = ratingSource(rating.source);
  const wordedScale = info === null ? rating.max !== 100 : info.style === "decimal";
  const fetched = rating.fetched_at_ms ?? 0;
  return [
    ratingSourceName(rating.source),
    wordedScale && rating.max > 0
      ? t("ratings.scale", { value: badgeValue(rating, format), max: rating.max })
      : "",
    rating.votes > 0 ? t("ratings.votes", { count: rating.votes }) : "",
    fetched > 0 ? t("ratings.fetched", { date: format.date(fetched) }) : "",
  ]
    .filter((part) => part !== "")
    .join(" · ");
}

/**
 * The name of a rating for screen readers and its link: the source, the value and the scale,
 * `IMDb 7.3 out of 10`, `Rotten Tomatoes critics 81%`.
 */
export function badgeLabel(rating: RatingBadgeData, format: Formatters): string {
  const info = ratingSource(rating.source);
  const wordedScale = info === null ? rating.max !== 100 : info.style === "decimal";
  const value = badgeValue(rating, format);
  const scaled =
    wordedScale && rating.max > 0 ? t("ratings.scale", { value, max: rating.max }) : value;
  return `${ratingSourceName(rating.source)} ${scaled}`;
}

/** The ratings in display order: the preferred source first, then the fallback order. */
export function orderedRatings(
  ratings: readonly RatingBadgeData[],
  preferred: string,
): RatingBadgeData[] {
  return [...ratings].sort(
    (a, b) =>
      ratingRank(preferred, a.source) - ratingRank(preferred, b.source) ||
      a.source.localeCompare(b.source),
  );
}
