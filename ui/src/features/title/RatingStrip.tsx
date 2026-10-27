import type { MouseEvent } from "react";

import type { DetailRatingDto } from "../../bindings";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { badgeHint, badgeLabel, badgeValue, orderedRatings } from "../ratings/ratingFormat";
import type { RatingBadgeData } from "../ratings/ratingFormat";
import { ratingPageUrl } from "../ratings/ratingLinks";
import type { RatingLinkTarget } from "../ratings/ratingLinks";
import { RatingMark } from "../ratings/ratingMarks";
import { openRatingPage } from "../ratings/ratingPage";
import { usePreferredSource } from "../ratings/preference";
import { ratingSourceName } from "../ratings/ratingSources";

interface RatingStripProps {
  ratings: readonly DetailRatingDto[];
  /** The title the ratings belong to: each rating opens the title's page on its source's site. */
  link: RatingLinkTarget;
}

/** The ratings that have a value, as the shared rating shape. */
function withValues(ratings: readonly DetailRatingDto[]): RatingBadgeData[] {
  return ratings.flatMap((rating) =>
    rating.value === null ? [] : [{ ...rating, value: rating.value }],
  );
}

/**
 * The ratings of a title page: each source's value at 16/700 with its mark beside it, 22px apart,
 * the preferred source first. A rating whose source names a page is a link that
 * opens it in the browser. Sources with no value are left out; Settings says why.
 */
export function RatingStrip({ ratings, link }: RatingStripProps) {
  const format = useFormat();
  const preferred = usePreferredSource();
  const shown = orderedRatings(withValues(ratings), preferred);
  if (shown.length === 0) return null;
  return (
    <ul
      aria-label={t("ratings.badges")}
      className="m-0 flex list-none flex-wrap gap-x-[22px] gap-y-2 p-0"
    >
      {shown.map((rating) => {
        const url = ratingPageUrl(rating.source, link, rating.url);
        const name = ratingSourceName(rating.source);
        const body = (
          <>
            <b
              aria-hidden="true"
              className="text-(length:--fs-16) font-(weight:--fw-700) tabular-nums"
            >
              {badgeValue(rating, format)}
            </b>
            <RatingMark source={rating.source} size={14} />
          </>
        );
        return (
          <li
            key={rating.source}
            data-source={rating.source}
            className="flex items-center gap-[7px]"
          >
            {url === null ? (
              <span title={badgeHint(rating, format)} className="flex items-center gap-[7px]">
                {body}
                <span className="sr-only">{badgeLabel(rating, format)}</span>
              </span>
            ) : (
              <a
                href={url}
                aria-label={badgeLabel(rating, format)}
                title={`${badgeHint(rating, format)}\n${t("ratings.link.openTitle", { source: name })}`}
                onClick={(event: MouseEvent<HTMLAnchorElement>) => {
                  event.preventDefault();
                  void openRatingPage(url, name);
                }}
                className="cv-hoverable flex items-center gap-[7px] text-inherit no-underline hover:opacity-80"
              >
                {body}
              </a>
            )}
          </li>
        );
      })}
    </ul>
  );
}
