import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useSetting } from "../settings/settingsStore";
import { useDebounced } from "../workspaces/useDebounced";
import { useProviderStream } from "../workspaces/useProviderStream";
import { isLinkOrId } from "./candidateText";
import { currentScrapeBackend } from "./scrapeBackend";
import type { ScrapeKind, ScrapeProviderInfo, ScrapeStreamHit } from "./scrapeBackend";
import { isCurrentMatch, mergeScrapeHits } from "./scrapeModel";

export interface CandidateSearchOptions {
  itemId: string;
  kind: ScrapeKind;
  /** The title to start with: the parsed title without its year. */
  title: string;
  year: number | null;
  /** Candidates to show before the providers are asked (a title already searched). */
  seed?: readonly ScrapeStreamHit[];
  /** The provider IDs the title has now (`tmdb:949`): its current match. */
  currentIds?: readonly string[];
  /**
   * The current match as an entry of its own, shown first when the search does not list it. The
   * search's own entry for it (with its score) takes its place when there is one.
   */
  currentHit?: ScrapeStreamHit | null;
}

const NO_SEED: readonly ScrapeStreamHit[] = [];
const NO_IDS: readonly string[] = [];

/** A year typed into the year field that the search can use. */
function usableYear(text: string): number | null {
  const value = text.trim();
  if (!/^\d{4}$/.test(value)) return null;
  const year = Number(value);
  return year >= 1880 && year <= 2100 ? year : null;
}

/**
 * The search of a picker or of the Scrape page: the typed title and year (or a pasted link or ID),
 * the Movie | TV show switch, the candidates streaming in from every provider as each answers,
 * and the one chosen. A title already searched shows its candidates at once and the providers are
 * asked when the user changes the search. The best match is chosen as soon as there is one, and a
 * better one that arrives later takes its place, until the user moves through the list.
 */
export function useCandidateSearch({
  itemId,
  kind,
  title,
  year,
  seed = NO_SEED,
  currentIds = NO_IDS,
  currentHit = null,
}: CandidateSearchOptions) {
  const backend = currentScrapeBackend();
  const language = useSetting<string>("metadata.language");
  const [text, setTextState] = useState(title);
  const [yearText, setYearTextState] = useState(year !== null && year > 0 ? String(year) : "");
  const [searchKind, setSearchKindState] = useState<ScrapeKind>(kind);
  const [touched, setTouched] = useState(false);

  // A pasted link or ID stands alone; a title takes the year with it ("The Batman (2022)").
  const typedYear = usableYear(yearText);
  const effective = isLinkOrId(text)
    ? text.trim()
    : typedYear === null
      ? text.trim()
      : `${text.trim()} (${typedYear})`;
  const query = useDebounced(effective);
  const searching = query !== "" && (touched || seed.length === 0);

  const [providers, setProviders] = useState<readonly ScrapeProviderInfo[]>([]);
  const transport = useMemo(
    () => backend.searchTransport((start) => setProviders(start.providers)),
    [backend],
  );
  const stream = useProviderStream({
    transport,
    query,
    params: { itemId, kind: searchKind, language },
    enabled: searching,
  });
  const found = useMemo(() => {
    if (!searching) return [...seed];
    const live = mergeScrapeHits(stream.items);
    // The candidates the title already has stay until the search has answered with its own.
    return live.length > 0 || seed.length === 0 || !stream.searching ? live : [...seed];
  }, [stream.items, stream.searching, seed, searching]);
  // The current match comes first: the search's own entry for it, else the stored one.
  const pinned = useMemo(
    () => found.find((hit) => isCurrentMatch(hit, currentIds)) ?? currentHit,
    [found, currentIds, currentHit],
  );
  const entries = useMemo(
    () => (pinned === null ? found : [pinned, ...found.filter((hit) => hit.id !== pinned.id)]),
    [found, pinned],
  );
  const providerNames = useMemo(
    () => Object.fromEntries(providers.map((entry) => [entry.key, entry.name])),
    [providers],
  );
  /** Why the search found nothing, in the backend's words; `undefined` while it still might. */
  const nothingFound = stream.providers.find((status) => status.provider === "search")?.reason;

  const [chosenId, setChosenId] = useState<string | null>(null);
  // The best other match is chosen at first; the current match only when it is all there is.
  const best = entries.find((hit) => hit.id !== pinned?.id) ?? entries[0] ?? null;
  const chosen =
    chosenId === null ? best : (entries.find((entry) => entry.id === chosenId) ?? best);
  const current = chosen !== null && pinned !== null && chosen.id === pinned.id;

  const entriesRef = useRef(entries);
  useEffect(() => {
    entriesRef.current = entries;
  });

  const choose = useCallback((hit: ScrapeStreamHit) => setChosenId(hit.id), []);
  /** Moves the choice through the list by one step; the best match stops following the results. */
  const move = useCallback((step: 1 | -1) => {
    const list = entriesRef.current;
    if (list.length === 0) return;
    setChosenId((previous) => {
      const at =
        previous === null
          ? 0
          : Math.max(
              0,
              list.findIndex((e) => e.id === previous),
            );
      return list[Math.min(list.length - 1, Math.max(0, at + step))]?.id ?? previous;
    });
  }, []);

  const changed = () => {
    setTouched(true);
    // A new search has a new best match.
    setChosenId(null);
  };

  return {
    text,
    setText: (next: string) => {
      setTextState(next);
      changed();
    },
    yearText,
    setYearText: (next: string) => {
      setYearTextState(next);
      changed();
    },
    kind: searchKind,
    setKind: (next: ScrapeKind) => {
      setSearchKindState(next);
      changed();
    },
    query,
    searching,
    stream,
    entries,
    providerNames,
    nothingFound,
    chosen,
    /** The current match, first in `entries`, or `null` when the title has none. */
    pinned,
    /** The chosen entry is the current match. */
    current,
    choose,
    move,
    /** The title typed is a pasted link or ID. */
    isLink: isLinkOrId(text),
  };
}

export type CandidateSearch = ReturnType<typeof useCandidateSearch>;
