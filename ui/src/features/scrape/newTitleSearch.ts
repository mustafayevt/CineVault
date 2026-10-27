import { useEffect, useRef } from "react";
import { create } from "zustand";

import { currentScrapeBackend } from "./scrapeBackend";
import type { ScrapeKind, ScrapeStreamHit } from "./scrapeBackend";

/** How many titles are searched at once; the backend's provider lanes pace the requests further. */
const SEARCH_AT_ONCE = 3;

/** What the search-only pass knows about one New title. */
export interface FoundCandidates {
  state: "searching" | "done";
  /** Best first; empty when the search found nothing. */
  hits: readonly ScrapeStreamHit[];
  /** The backend's verdict: the top candidate scores 90% or more and is not tied. */
  confident: boolean;
}

interface FoundState {
  found: Readonly<Record<string, FoundCandidates>>;
  set: (itemId: string, value: FoundCandidates | null) => void;
}

/**
 * The candidates found for New titles while the Scrape page is open, by library item. They are
 * kept for the session, so coming back to the page does not search the same titles again; nothing
 * here is written to a title, its NFO or its artwork.
 */
export const useFoundCandidates = create<FoundState>((set) => ({
  found: {},
  set: (itemId, value) =>
    set((state) => {
      if (value === null) {
        return {
          found: Object.fromEntries(Object.entries(state.found).filter(([id]) => id !== itemId)),
        };
      }
      return { found: { ...state.found, [itemId]: value } };
    }),
}));

/** A New title to search for: the text is its parsed title and year. */
export interface SearchJob {
  itemId: string;
  kind: ScrapeKind;
  query: string;
}

/**
 * The search-only pass for the New titles of the Scrape page (spec `title-matching` "New titles
 * are searched while the Scrape page is open"): a few titles at a time, in the order given (the
 * ones on screen first), until the page closes. The backend searches and scores and writes
 * nothing; its provider lanes pace the requests further.
 */
export function useNewTitleSearch(
  jobs: readonly SearchJob[],
  language: string,
  enabled: boolean,
): void {
  const found = useFoundCandidates((state) => state.found);
  const running = useRef(new Set<string>());
  const closed = useRef(false);

  useEffect(() => {
    if (!enabled) return;
    const backend = currentScrapeBackend();
    if (!backend.available) return;
    const room = SEARCH_AT_ONCE - running.current.size;
    if (room <= 0) return;
    const store = useFoundCandidates.getState();
    const next = jobs
      .filter((job) => job.query !== "" && !running.current.has(job.itemId))
      .filter((job) => store.found[job.itemId] === undefined)
      .slice(0, room);
    for (const job of next) {
      running.current.add(job.itemId);
      store.set(job.itemId, { state: "searching", hits: [], confident: false });
      const finish = (hits: readonly ScrapeStreamHit[], confident: boolean) => {
        running.current.delete(job.itemId);
        if (closed.current) return;
        useFoundCandidates.getState().set(job.itemId, { state: "done", hits, confident });
      };
      void backend.searchOnly(job.itemId, job.kind, job.query, language).then(
        (result) => finish(result.hits, result.confident),
        // A search that failed shows as found nothing; the title can still be searched by hand.
        () => finish([], false),
      );
    }
    // `found` is a dependency so a finished search makes room for the next one.
  }, [jobs, language, enabled, found]);

  // Leaving the page stops the searching: no further searches start and the answers of the
  // running ones are dropped.
  useEffect(() => {
    closed.current = false;
    const active = running.current;
    return () => {
      closed.current = true;
      for (const itemId of active) useFoundCandidates.getState().set(itemId, null);
      active.clear();
    };
  }, []);
}
