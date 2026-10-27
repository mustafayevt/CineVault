import { useVirtualizer } from "@tanstack/react-virtual";
import { useQueryClient } from "@tanstack/react-query";
import { Link, useSearch } from "@tanstack/react-router";
import { Check, KeyRound } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Skeleton } from "../../components/Skeleton";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useKeyAttention } from "../providers/useKeyAttention";
import { useSetting } from "../settings/settingsStore";
import { CandidatePane } from "./CandidatePane";
import { useFoundCandidates, useNewTitleSearch } from "./newTitleSearch";
import { QUEUE_ROW_GAP, QUEUE_ROW_HEIGHT, QueueRow } from "./QueueRow";
import { acceptConfident, scrapeAndReport } from "./scrapeApply";
import type { ScrapeSubjectRef } from "./scrapeApply";
import type { QueueEntry, QueueTab } from "./scrapeQueue";
import { scopeIdsOf } from "./scrapeSearch";
import type { ScrapeSearch } from "./scrapeSearch";
import type { ScrapeStreamHit } from "./scrapeBackend";
import { useScopedQueue, useScrapeQueue } from "./useScrapeQueue";
import type { ScrapeQueue } from "./useScrapeQueue";

const NO_HITS: readonly ScrapeStreamHit[] = [];

function subjectOf(entry: QueueEntry): ScrapeSubjectRef {
  return {
    itemId: entry.itemId,
    kind: entry.kind,
    title: entry.title,
    year: entry.year,
    reviewId: entry.review?.id ?? null,
  };
}

/** Nothing waits, or the key that scraping needs is missing. */
function EmptyScrape() {
  const attention = useKeyAttention();
  const paused = attention?.identification_paused === true;
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
      <Icon icon={paused ? KeyRound : Check} size={24} className="text-text-muted" />
      <h2 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
        {paused ? t("scrapePage.noKey.title") : t("scrapePage.empty.title")}
      </h2>
      <p className="m-0 max-w-[44ch] text-text-2">
        {paused ? t("scrapePage.noKey.body") : t("scrapePage.empty.body")}
      </p>
      {paused ? (
        <Link
          to="/settings/providers"
          className="cv-hoverable inline-flex h-(--h-btn) items-center rounded-(--r-6) border border-solid border-line-edge bg-surface-raised px-3 text-text no-underline hover:bg-surface-active"
        >
          {t("scrapePage.noKey.action")}
        </Link>
      ) : (
        <Link to="/library" className="cv-link text-(length:--fs-13)">
          {t("scrapePage.empty.back")}
        </Link>
      )}
    </div>
  );
}

interface ScrapeViewProps {
  queue: ScrapeQueue;
  tab: QueueTab;
  onTab: (tab: QueueTab) => void;
}

/** The Scrape page body for a queue: the tabs, the queue list and the candidates of the current title. */
function ScrapeView({ queue, tab, onTab }: ScrapeViewProps) {
  const queryClient = useQueryClient();
  const format = useFormat();
  const language = useSetting<string>("metadata.language");
  const found = useFoundCandidates((state) => state.found);
  const { slots } = queue;
  const keys = useMemo(() => slots.map((slot) => slot.key), [slots]);
  const selection = useListSelection<string>({ keys });
  const shownKey =
    selection.current !== null && keys.includes(selection.current)
      ? selection.current
      : (keys[0] ?? null);
  const shown = slots.find((slot) => slot.key === shownKey)?.entry ?? null;
  const { hitsOf } = queue;
  const shownHits = useMemo(() => (shown === null ? NO_HITS : hitsOf(shown)), [shown, hitsOf]);

  // The rows on screen: the library loads their pages and the search-only pass starts there.
  const scrollRef = useRef<HTMLDivElement>(null);
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: slots.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => QUEUE_ROW_HEIGHT + QUEUE_ROW_GAP,
    overscan: 6,
  });
  const items = virtualizer.getVirtualItems();
  const first = items[0]?.index ?? 0;
  const last = items[items.length - 1]?.index ?? 0;
  const [range, setRange] = useState({ start: 0, end: 12 });
  useEffect(() => {
    setRange((previous) =>
      previous.start === first && previous.end === last ? previous : { start: first, end: last },
    );
  }, [first, last]);
  const { setRange: loadRange } = queue;
  useEffect(() => loadRange(range), [loadRange, range]);
  const jobs = useMemo(() => queue.jobsFor(range), [queue, range]);
  useNewTitleSearch(jobs, language, true);

  // The current row stays in view as the keys move through the list.
  const shownIndex = shownKey === null ? -1 : keys.indexOf(shownKey);
  useEffect(() => {
    if (shownIndex >= 0) virtualizer.scrollToIndex(shownIndex, { align: "auto" });
  }, [shownIndex, virtualizer]);

  const neighbourOf = useCallback(
    (key: string): string | null => {
      const index = keys.indexOf(key);
      return keys[index + 1] ?? keys[index - 1] ?? null;
    },
    [keys],
  );

  const scrapeHit = useCallback(
    (entry: QueueEntry, hit: ScrapeStreamHit) => {
      const next = neighbourOf(entry.key);
      queue.hide([entry.itemId]);
      selection.select(next === null ? [] : [next]);
      void scrapeAndReport(queryClient, subjectOf(entry), hit, language).then((ok) => {
        if (ok) return;
        // The scrape was refused and rolled back: the title is back in the list.
        queue.show([entry.itemId]);
        selection.select([entry.key]);
      });
    },
    [language, neighbourOf, queue, queryClient, selection],
  );

  const skip = useCallback(() => {
    if (shownKey === null) return;
    const index = keys.indexOf(shownKey);
    const next = keys[index + 1];
    if (next !== undefined) selection.select([next]);
  }, [keys, selection, shownKey]);

  const confidentCount = queue.confident.length;
  const accepting = useRef(false);
  const accept = useCallback(() => {
    if (accepting.current || queue.confident.length === 0) return;
    accepting.current = true;
    const entries = [...queue.confident];
    queue.hide(entries.map((entry) => entry.itemId));
    void acceptConfident(
      queryClient,
      entries.map((entry) => entry.itemId),
    )
      .then(({ failed }) => {
        if (failed.length > 0) queue.show(failed);
      })
      .finally(() => {
        accepting.current = false;
      });
  }, [queue, queryClient]);

  const total = queue.counts.new + queue.counts.uncertain + queue.counts.notMatched;
  const everythingEmpty =
    !queue.loading && queue.error === null && total === 0 && slots.length === 0;
  const tabCount = (id: QueueTab): number =>
    id === "all"
      ? total
      : id === "new"
        ? queue.counts.new
        : id === "uncertain"
          ? queue.counts.uncertain
          : queue.counts.notMatched;

  return (
    // The page itself takes the Down key (skip) only when nothing inside it has the focus.
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
    <section
      aria-label={t("scrapePage.region")}
      onKeyDown={(event) => {
        // Down skips to the next title when nothing in the page takes the key itself.
        if (event.key === "ArrowDown" && event.target === event.currentTarget) {
          event.preventDefault();
          skip();
        }
      }}
      className="relative flex h-full min-h-0 flex-col outline-none"
    >
      <h1 className="sr-only">{t("scrapePage.region")}</h1>
      <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
        <span
          aria-hidden="true"
          className="absolute right-6 bottom-0 left-6 h-px bg-line-section"
        />
        <SegmentedControl<QueueTab>
          aria-label={t("scrapePage.tabs")}
          value={tab}
          onChange={onTab}
          options={[
            { value: "all", label: t("scrapePage.tab.all"), count: format.number(tabCount("all")) },
            { value: "new", label: t("scrapePage.tab.new"), count: format.number(tabCount("new")) },
            {
              value: "uncertain",
              label: t("scrapePage.tab.uncertain"),
              count: format.number(tabCount("uncertain")),
            },
            {
              value: "notMatched",
              label: t("scrapePage.tab.notMatched"),
              count: format.number(tabCount("notMatched")),
            },
          ]}
        />
        <span className="grow" />
        <Button
          icon={Check}
          title={t("scrapePage.accept.tip")}
          {...(confidentCount === 0 ? { disabledReason: t("scrapePage.accept.none") } : {})}
          onClick={accept}
        >
          {t("scrapePage.accept", { count: confidentCount })}
        </Button>
      </div>

      {everythingEmpty ? (
        <EmptyScrape />
      ) : (
        <div className="flex min-h-0 flex-1">
          <div className="flex w-[330px] shrink-0 flex-col border-r border-solid border-line-section">
            {queue.error !== null ? (
              <div role="alert" className="flex flex-col items-start gap-2 px-6 py-3">
                <p className="m-0 text-danger">{t("scrapePage.queue.loadFailed")}</p>
                <Button onClick={queue.refetch}>{t("scrapePage.queue.retry")}</Button>
              </div>
            ) : queue.loading && slots.length === 0 ? (
              <div aria-busy="true" className="flex flex-col gap-1 px-6 py-3">
                {[0, 1, 2, 3].map((n) => (
                  <Skeleton key={n} shape="line" width="100%" height="72px" />
                ))}
              </div>
            ) : slots.length === 0 ? (
              <p className="m-0 px-6 py-4 text-text-muted">{t("scrapePage.queue.empty")}</p>
            ) : (
              // The list holds the focus and points at the current row, so rows can come and go.
              <div
                ref={scrollRef}
                role="listbox"
                aria-label={t("scrapePage.queue")}
                aria-activedescendant={shownKey === null ? undefined : `queue-${shownKey}`}
                tabIndex={0}
                onKeyDown={(event) => {
                  if (selection.onKeyDown(event)) event.preventDefault();
                }}
                className="min-h-0 flex-1 overflow-auto px-6 py-3 outline-none"
              >
                <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
                  {items.map((item) => {
                    const slot = slots[item.index];
                    if (slot === undefined) return null;
                    const entry = slot.entry;
                    return (
                      <div
                        key={slot.key}
                        className="absolute top-0 left-0 w-full"
                        style={{ transform: `translateY(${item.start}px)` }}
                      >
                        <QueueRow
                          id={`queue-${slot.key}`}
                          entry={entry}
                          hits={entry === null ? [] : queue.hitsOf(entry)}
                          searching={entry === null ? undefined : found[entry.itemId]?.state}
                          current={slot.key === shownKey}
                          onClick={() => selection.click(slot.key)}
                        />
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
          <div className="flex min-w-0 flex-1 flex-col">
            {shown === null ? (
              <p className="m-0 px-6 py-4 text-text-muted">{t("scrapePage.pick.empty")}</p>
            ) : (
              <CandidatePane
                key={shown.key}
                entry={shown}
                hits={shownHits}
                onScrape={(hit) => scrapeHit(shown, hit)}
                onSkip={skip}
              />
            )}
          </div>
        </div>
      )}
    </section>
  );
}

function FullQueue() {
  const [tab, setTab] = useState<QueueTab>("all");
  const queue = useScrapeQueue(tab);
  return <ScrapeView queue={queue} tab={tab} onTab={setTab} />;
}

function ScopedQueue({ ids }: { ids: readonly string[] }) {
  const [tab, setTab] = useState<QueueTab>("all");
  const queue = useScopedQueue(tab, ids);
  return <ScrapeView queue={queue} tab={tab} onTab={setTab} />;
}

/**
 * The Scrape page (`/scrape`): the tabs All, New, Uncertain and Not matched with their
 * counts, "Accept all confident (N)", the queue of titles waiting for a scrape decision on the left
 * and the candidates of the current title on the right. The Library sends a selection as
 * `?ids=`; the queue is then those titles.
 */
export function ScrapePage() {
  const search = useSearch({ strict: false }) as ScrapeSearch;
  const ids = scopeIdsOf(search);
  // The route changes, not the queue, when the selection does: a new selection starts fresh.
  return ids === null ? <FullQueue /> : <ScopedQueue key={ids.join(",")} ids={ids} />;
}
