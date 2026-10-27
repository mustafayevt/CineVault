import { useEffect, useMemo, useState } from "react";

import { currentLibraryBackend } from "../library/libraryBackend";
import type { LibraryCard, LibraryKind, PersonHit } from "../library/libraryBackend";
import { parseQuery } from "./queryParser";

/** The pause after the last keystroke before the library is asked. */
export const SPOTLIGHT_PAUSE_MS = 80;
/** Rows asked of each group: a few more than shown, so the top hit can leave its group. */
const ASKED = 7;
const PEOPLE = 4;

export interface SpotlightResults {
  /** The text these results are for. */
  term: string;
  movies: readonly LibraryCard[];
  shows: readonly LibraryCard[];
  collections: readonly LibraryCard[];
  people: readonly PersonHit[];
  /** Every title and collection that matches, not only the ones listed. */
  total: number;
}

async function ask(term: string): Promise<SpotlightResults> {
  const backend = currentLibraryBackend();
  const listed = async (kind: LibraryKind) => {
    try {
      return await backend.window({ kind, sort: [], search: term, filters: [] }, 0, ASKED);
    } catch {
      return { total: 0, offset: 0, items: [] as LibraryCard[] };
    }
  };
  const [movies, shows, collections, people] = await Promise.all([
    listed("movies"),
    listed("shows"),
    listed("collections"),
    backend.people === undefined
      ? Promise.resolve([] as PersonHit[])
      : backend.people(term, PEOPLE).catch(() => [] as PersonHit[]),
  ]);
  return {
    term,
    movies: movies.items,
    shows: shows.items,
    collections: collections.items,
    people,
    total: movies.total + shows.total + collections.total,
  };
}

export interface SpotlightState {
  /** The last results that arrived for the free text, kept while a newer search is out. */
  results: SpotlightResults | null;
  /** A search for the current text is still out. */
  busy: boolean;
  /** The text holds a filter (`year:2010`), which the list of suggestions does not search. */
  structured: boolean;
}

/**
 * The library search behind the title-bar suggestions: asks the forgiving backend search for the
 * movies, shows, collections and people that match the text, 80 ms after the last keystroke. A
 * request that was replaced is ignored when it answers, and the last results stay in place until
 * the next ones arrive, so the list never blinks. Text with a filter in it is left to the
 * Library page.
 */
export function useSpotlightResults(text: string): SpotlightState {
  const parsed = useMemo(() => parseQuery(text), [text]);
  const structured = parsed.chips.length > 0 || parsed.rejected.length > 0;
  const wanted = structured ? "" : parsed.text.trim();
  const [results, setResults] = useState<SpotlightResults | null>(null);
  const [settled, setSettled] = useState("");

  // Nothing to look up: forget the old results, so the next text starts clean.
  if (wanted === "" && (results !== null || settled !== "")) {
    setResults(null);
    setSettled("");
  }

  useEffect(() => {
    if (wanted === "") return;
    let stale = false;
    const timer = window.setTimeout(() => {
      void ask(wanted)
        .then((found) => {
          if (stale) return;
          setResults(found);
          setSettled(wanted);
        })
        .catch(() => {
          if (!stale) setSettled(wanted);
        });
    }, SPOTLIGHT_PAUSE_MS);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [wanted]);

  return { results, busy: wanted !== "" && settled !== wanted, structured };
}
