import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";

import type { LibraryChip, LibraryQuery } from "../library/libraryBackend";
import { useLibraryListing } from "../library/useLibraryListing";
import type { Listing } from "../library/useLibraryListing";
import { useFoundCandidates } from "./newTitleSearch";
import type { FoundCandidates, SearchJob } from "./newTitleSearch";
import { currentReviewBackend } from "./reviewBackend";
import type { ReviewItem } from "./reviewBackend";
import type { ScrapeStreamHit } from "./scrapeBackend";
import {
  entryOfNew,
  entryOfReview,
  entryOfSubject,
  inTab,
  isConfident,
  seedHits,
} from "./scrapeQueue";
import type { QueueEntry, QueueStatus, QueueTab } from "./scrapeQueue";
import { loadTitleSubject, titleSubjectKey } from "./titleSubject";

/** Query key of the review rows (Uncertain and Not matched titles). */
export const REVIEW_QUEUE_KEY = ["review", "queue"] as const;
/** Query key of the number of review rows. */
export const REVIEW_COUNT_KEY = ["review", "count"] as const;

const chip = (field: LibraryChip["field"], values: string[], negated = false): LibraryChip => ({
  field,
  negated,
  values,
  min: null,
  max: null,
});

/**
 * The New titles of a library type: scraped never, and not waiting as a review row either (those
 * are the Uncertain and Not matched tabs), in title order.
 */
function newQuery(kind: "movies" | "shows"): LibraryQuery {
  return {
    kind,
    sort: [{ field: "title", descending: false }],
    search: "",
    filters: [chip("stage", ["new"]), chip("review", [], true), chip("not_matched", [], true)],
  };
}

/** One line of the queue: its entry once known (a New title's page may still be loading). */
export interface QueueSlot {
  key: string;
  status: QueueStatus;
  entry: QueueEntry | null;
  /** Where a New title's row comes from, to ask its page to load. */
  source: { listing: "movies" | "shows"; index: number } | null;
}

/** What the Scrape page reads from its queue, whether it is the whole queue or a selection. */
export interface ScrapeQueue {
  loading: boolean;
  error: Error | null;
  refetch: () => void;
  /** The lines of the open tab. */
  slots: readonly QueueSlot[];
  counts: { new: number; uncertain: number; notMatched: number };
  /** The candidates known for an entry. */
  hitsOf: (entry: QueueEntry) => readonly ScrapeStreamHit[];
  /** The titles whose top candidate is confident, over the whole queue and not only the tab. */
  confident: readonly QueueEntry[];
  /** Tells the lists which lines are on screen, so their pages load. */
  setRange: (range: { start: number; end: number }) => void;
  /** The New titles to search, the ones around `range` first. */
  jobsFor: (range: { start: number; end: number }) => SearchJob[];
  hide: (itemIds: readonly string[]) => void;
  show: (itemIds: readonly string[]) => void;
}

const NO_REVIEW: readonly ReviewItem[] = [];
const NO_HITS: readonly ScrapeStreamHit[] = [];

function countsOf(slots: readonly QueueSlot[]) {
  const count = { new: 0, uncertain: 0, notMatched: 0 };
  for (const slot of slots) {
    if (slot.status !== "scraped") count[slot.status] += 1;
  }
  return count;
}

/** The candidates known for an entry: a review row's own, or what the search-only pass found. */
function useHitsOf() {
  const found = useFoundCandidates((state) => state.found);
  return useCallback(
    (entry: QueueEntry): readonly ScrapeStreamHit[] => {
      if (entry.review !== null) return seedHits(entry.review);
      return found[entry.itemId]?.hits ?? NO_HITS;
    },
    [found],
  );
}

/**
 * The titles Accept all confident takes: a review row whose top candidate is confident, and a New
 * title the search-only pass found confident (the backend's verdict, so the count is what the
 * button does).
 */
function confidentOf(
  slots: readonly QueueSlot[],
  found: Readonly<Record<string, FoundCandidates>>,
): QueueEntry[] {
  const list: QueueEntry[] = [];
  for (const slot of slots) {
    const entry = slot.entry;
    if (entry === null) continue;
    const confident =
      entry.review !== null
        ? isConfident(seedHits(entry.review), entry.review.tied)
        : found[entry.itemId]?.confident === true;
    if (confident) list.push(entry);
  }
  return list;
}

function jobsOf(slots: readonly QueueSlot[], range: { start: number; end: number }): SearchJob[] {
  const near = slots.slice(Math.max(0, range.start - 2), range.end + 8);
  const rest = slots.filter((slot) => !near.includes(slot));
  return [...near, ...rest.slice(0, 40)].flatMap((slot) => {
    const entry = slot.entry;
    if (entry === null || entry.status !== "new") return [];
    const query = entry.year === null ? entry.title : `${entry.title} (${entry.year})`;
    return [{ itemId: entry.itemId, kind: entry.kind, query }];
  });
}

function useHidden() {
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const hide = useCallback((itemIds: readonly string[]) => {
    setHidden((current) => new Set([...current, ...itemIds]));
  }, []);
  const show = useCallback((itemIds: readonly string[]) => {
    setHidden((current) => {
      const next = new Set(current);
      for (const id of itemIds) next.delete(id);
      return next;
    });
  }, []);
  return { hidden, hide, show };
}

/**
 * The queue of the Scrape page (spec `title-matching` "Needs review inbox"): New titles never
 * scraped, then the Uncertain and Not matched review rows. New titles come from the library as a
 * virtual list, so thousands of them cost one page of rows at a time; review rows are read whole.
 * Titles that were just scraped are hidden at once, before the lists are read again.
 */
export function useScrapeQueue(tab: QueueTab): ScrapeQueue {
  const review = useQuery({
    queryKey: REVIEW_QUEUE_KEY,
    queryFn: () => currentReviewBackend().list(),
  });
  const movies = useLibraryListing(newQuery("movies"));
  const shows = useLibraryListing(newQuery("shows"));
  const { hidden, hide, show } = useHidden();
  const hitsOf = useHitsOf();
  const found = useFoundCandidates((state) => state.found);
  const reviewItems = review.data ?? NO_REVIEW;

  const slots = useMemo<QueueSlot[]>(() => {
    const reviewSlots: QueueSlot[] = [];
    for (const item of reviewItems) {
      if (hidden.has(item.itemId)) continue;
      const entry = entryOfReview(item);
      reviewSlots.push({ key: entry.key, status: entry.status, entry, source: null });
    }
    const newSlots = (listing: Listing, name: "movies" | "shows"): QueueSlot[] => {
      const result: QueueSlot[] = [];
      listing.keys.forEach((id, index) => {
        if (hidden.has(id)) return;
        const row = listing.rowAt(index);
        const key = `${name === "movies" ? "m" : "s"}:${id}`;
        result.push({
          key,
          status: "new",
          entry:
            row === undefined ? null : entryOfNew(row, name === "movies" ? "movie" : "show", key),
          source: { listing: name, index },
        });
      });
      return result;
    };
    return [
      ...newSlots(movies, "movies"),
      ...newSlots(shows, "shows"),
      ...reviewSlots.filter((slot) => slot.status === "uncertain"),
      ...reviewSlots.filter((slot) => slot.status === "notMatched"),
    ];
    // The listings change identity every render; their `version` says when a page arrived.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reviewItems, hidden, movies.version, movies.total, shows.version, shows.total]);

  const counts = useMemo(() => countsOf(slots), [slots]);
  const visible = useMemo(() => slots.filter((slot) => inTab(slot, tab)), [slots, tab]);
  const confident = useMemo(() => confidentOf(slots, found), [slots, found]);

  const setRange = useCallback(
    (range: { start: number; end: number }) => {
      const indexes = { movies: [] as number[], shows: [] as number[] };
      for (let i = range.start; i <= range.end; i += 1) {
        const source = visible[i]?.source;
        if (source !== null && source !== undefined) indexes[source.listing].push(source.index);
      }
      if (indexes.movies.length > 0) {
        movies.setRange({ start: Math.min(...indexes.movies), end: Math.max(...indexes.movies) });
      }
      if (indexes.shows.length > 0) {
        shows.setRange({ start: Math.min(...indexes.shows), end: Math.max(...indexes.shows) });
      }
    },
    [visible, movies, shows],
  );
  const jobsFor = useCallback(
    (range: { start: number; end: number }) => jobsOf(visible, range),
    [visible],
  );

  return {
    loading: review.isPending || movies.loading || shows.loading,
    error: review.error ?? movies.error ?? shows.error,
    refetch: () => void review.refetch(),
    slots: visible,
    counts,
    hitsOf,
    confident,
    setRange,
    jobsFor,
    hide,
    show,
  };
}

/** The most titles a selection sent from the Library lists; the rest wait for the next round. */
const SCOPE_LIMIT = 200;

/**
 * The queue of a selection (`/scrape?ids=`): the titles the Library sent, each with its review
 * row when it has one. Titles that already have a match are listed under All.
 */
export function useScopedQueue(tab: QueueTab, ids: readonly string[]): ScrapeQueue {
  const queryClient = useQueryClient();
  const review = useQuery({
    queryKey: REVIEW_QUEUE_KEY,
    queryFn: () => currentReviewBackend().list(),
  });
  const { hidden, hide, show } = useHidden();
  const hitsOf = useHitsOf();
  const found = useFoundCandidates((state) => state.found);
  const scoped = useMemo(() => ids.slice(0, SCOPE_LIMIT), [ids]);
  const subjects = useQueries({
    queries: scoped.map((id) => ({
      queryKey: titleSubjectKey(id),
      queryFn: () => loadTitleSubject(queryClient, id),
      staleTime: 0,
    })),
  });
  const arrivals = subjects.map((result) => result.dataUpdatedAt).join();
  const reviewItems = review.data ?? NO_REVIEW;

  const slots = useMemo<QueueSlot[]>(
    () =>
      subjects.flatMap((result, index) => {
        const id = scoped[index] ?? "";
        const subject = result.data;
        if (subject === undefined || hidden.has(id)) return [];
        const row = reviewItems.find((item) => item.itemId === id) ?? null;
        const entry = entryOfSubject(subject, row);
        return [{ key: entry.key, status: entry.status, entry, source: null }];
      }),
    // The query results change identity every render; their arrival times say when they changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [scoped, arrivals, hidden, reviewItems],
  );
  const counts = useMemo(() => countsOf(slots), [slots]);
  const visible = useMemo(() => slots.filter((slot) => inTab(slot, tab)), [slots, tab]);
  const confident = useMemo(() => confidentOf(slots, found), [slots, found]);
  const jobsFor = useCallback(
    (range: { start: number; end: number }) => jobsOf(visible, range),
    [visible],
  );

  return {
    loading: review.isPending || subjects.some((result) => result.isPending),
    error: review.error ?? subjects.find((result) => result.error !== null)?.error ?? null,
    refetch: () => void review.refetch(),
    slots: visible,
    counts,
    hitsOf,
    confident,
    setRange: () => undefined,
    jobsFor,
    hide,
    show,
  };
}
