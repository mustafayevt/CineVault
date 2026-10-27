import { useMemo } from "react";

import { useDebounced } from "../workspaces/useDebounced";
import { noProviderAnswered } from "../workspaces/providerOutcome";
import { useProviderStream } from "../workspaces/useProviderStream";
import { currentTorrentBackend } from "./torrentBackend";
import type { TitleHit } from "./torrentBackend";
import { mergeTitleHits } from "./torrentModel";

/** The fewest characters that start a search. */
const MIN_QUERY = 2;
/** The pause after the last keystroke before a search starts (spec `torrent-search`). */
const TYPING_PAUSE_MS = 400;

export interface TitleMatch {
  /** The titles found, best match first; hits of equal fit keep their arrival order. */
  hits: readonly TitleHit[];
  /** The text is too short to search. */
  short: boolean;
  /** The pause after typing is over and every source has answered. */
  settled: boolean;
  /** No source answered at all. */
  failed: boolean;
  /** Asks the sources that failed again. */
  retry: () => void;
}

/**
 * The title search of the Torrents page: after a short pause in typing, looks the text up (a title,
 * `Title (Year)`, an IMDb or TMDB ID or a link) and returns the matches best first. The page takes
 * the first one and offers the rest as "pick another".
 */
export function useTitleMatch(text: string, enabled: boolean): TitleMatch {
  const transport = useMemo(() => currentTorrentBackend().titleTransport(), []);
  const typed = text.trim();
  const query = useDebounced(typed, TYPING_PAUSE_MS);
  const short = typed.length < MIN_QUERY;
  // The search runs once the typing has paused, so no request is made for text already replaced.
  const active = enabled && !short && query === typed;
  const stream = useProviderStream<TitleHit>({ transport, query, enabled: active });
  const hits = useMemo(() => (short ? [] : mergeTitleHits(stream.items)), [short, stream.items]);
  const { providers, retry } = stream;
  return {
    hits,
    short,
    settled: active && !stream.searching,
    failed: active && !stream.searching && noProviderAnswered(providers),
    retry: () => {
      for (const status of providers) {
        if (status.state !== "searching" && status.state !== "done") retry(status.provider);
      }
    },
  };
}
