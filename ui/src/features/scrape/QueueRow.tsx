import type { MouseEvent } from "react";

import { ScoreBadge } from "../../components/ScoreBadge";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import type { FoundCandidates } from "./newTitleSearch";
import type { ScrapeStreamHit } from "./scrapeBackend";
import { hitName } from "./scrapeModel";
import { eitherName, isConfident, percentOf, scoreTone } from "./scrapeQueue";
import type { QueueEntry } from "./scrapeQueue";

/** The fixed height of a queue row, so the list scrolls without measuring. */
export const QUEUE_ROW_HEIGHT = 72;
/** The gap between rows. */
export const QUEUE_ROW_GAP = 2;

interface QueueRowProps {
  id: string;
  /** The title, or `null` while its page of the library is still loading. */
  entry: QueueEntry | null;
  hits: readonly ScrapeStreamHit[];
  /** The state of the search-only pass for a New title. */
  searching: FoundCandidates["state"] | undefined;
  current: boolean;
  onClick: (event: MouseEvent) => void;
}

/** The line under the file: what the title would become, or why there is nothing yet. */
function pickLine(
  entry: QueueEntry,
  hits: readonly ScrapeStreamHit[],
  searching: boolean,
  done: boolean,
) {
  const [first] = hits;
  if (first === undefined) {
    if (entry.status === "scraped")
      return { text: t("scrapePage.row.scraped", { name: entry.name }) };
    if (entry.review !== null) {
      return {
        text: entry.review.reason !== "" ? entry.review.reason : t("scrapePage.row.notMatched"),
      };
    }
    if (searching) return { text: t("scrapePage.row.searching") };
    if (done) return { text: t("scrapePage.row.noCandidates") };
    return { text: t("scrapePage.row.waiting") };
  }
  if (isConfident(hits, entry.review?.tied === true)) return { text: hitName(first.candidate) };
  const either = eitherName(hits);
  return {
    text:
      either === null
        ? t("scrapePage.row.maybe", { name: hitName(first.candidate) })
        : t("scrapePage.row.either", {
            first: hitName(first.candidate),
            second: hitName(hits[1]?.candidate ?? first.candidate),
          }),
  };
}

/**
 * One title of the Scrape queue: its name with the score of the best candidate, the
 * file in the mono face, and the match it would get. The current row takes the selection tint and
 * the 2px gold edge.
 */
export function QueueRow({ id, entry, hits, searching, current, onClick }: QueueRowProps) {
  if (entry === null) {
    return (
      <div
        id={id}
        role="option"
        aria-selected={false}
        aria-busy="true"
        className="flex h-[72px] flex-col gap-1 rounded-(--r-6) px-2.5 py-2"
      >
        <span className="h-[18px] w-3/5 rounded-(--r-4) bg-surface-active opacity-60" />
        <span className="h-[14px] w-4/5 rounded-(--r-4) bg-surface-active opacity-40" />
      </div>
    );
  }
  const top = hits[0];
  const percent = top === undefined ? null : percentOf(top.candidate.confidence);
  const pick = pickLine(entry, hits, searching === "searching", searching === "done");
  const pending = percent === null && entry.status === "new" && searching !== "done";
  return (
    // The list holds the focus and points at the current row (aria-activedescendant).
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus
    <div
      id={id}
      role="option"
      aria-selected={current}
      data-status={entry.status}
      data-current={current ? "true" : undefined}
      onClick={onClick}
      className={cx(
        "cv-hoverable flex h-[72px] cursor-pointer flex-col gap-[3px] rounded-(--r-6) px-2.5 py-2",
        current
          ? "bg-selected-row shadow-[inset_2px_0_0_var(--gold)]"
          : "hover:bg-surface-card active:bg-surface-raised",
      )}
    >
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate font-(weight:--fw-600)">{entry.name}</span>
        {percent !== null ? (
          <ScoreBadge
            value={t("scrapePage.row.score", { percent })}
            source={t("scrapePage.row.topMatch")}
            tone={scoreTone(percent)}
          />
        ) : pending ? (
          <ScoreBadge value={t("scrapePage.row.noScore")} />
        ) : entry.status === "scraped" ? null : (
          <ScoreBadge
            value={t("scrapePage.row.noScore")}
            source={t("scrapePage.row.topMatch")}
            tone="bad"
          />
        )}
      </div>
      <div
        title={entry.path}
        className="cv-selectable truncate font-mono text-(length:--fs-11) text-text-muted"
      >
        {entry.file}
      </div>
      <div className="truncate text-(length:--fs-12) text-text-2">{pick.text}</div>
    </div>
  );
}
