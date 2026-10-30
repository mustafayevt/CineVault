import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { ProviderEvent } from "../workspaces/providerEvents";
import { TorrentRequestError, currentTorrentBackend } from "./torrentBackend";
import type { FeedItem, FeedKind, FeedRow, FeedSource, FeedStart } from "./torrentBackend";

/** Where a source of the feed stands, as the page shows it. */
export type FeedSourceState =
  "asked" | "more" | "loading" | "done" | "unsupported" | "movies-only" | "tv-only" | "failed";

export interface FeedSourceView {
  id: string;
  name: string;
  kind: string;
  state: FeedSourceState;
  /** The reason a failed source gave. */
  reason?: string;
}

/** The feed of one kind as the page holds it. */
export interface FeedData {
  generation: string | null;
  /** The rows by release id, in the order they first arrived. */
  rows: ReadonlyMap<string, FeedRow>;
  sources: readonly FeedSourceView[];
  /** Requests started and not finished. */
  running: number;
  /** Whether the first page was asked at least once. */
  started: boolean;
  /** A "more" that added nothing while a source failed: no automatic next page until asked. */
  paused: boolean;
}

export interface LatestFeed {
  /** The rows, in arrival order. */
  rows: readonly FeedRow[];
  sources: readonly FeedSourceView[];
  /** A request is running. */
  loading: boolean;
  /** The first page is still coming and nothing has arrived. */
  firstLoad: boolean;
  /** Some source has a next page (or failed and can be asked again). */
  hasMore: boolean;
  /** Automatic paging stopped after a failed page; "Load more" asks again. */
  paused: boolean;
  /** Reads the next page of every source that has one. */
  loadMore: () => void;
  /** Forgets the rows and reads every source's first page again. */
  refresh: () => void;
}

/** How often, and how far apart, a request refused for the notice is asked again. */
const NOTICE_WAITS = 5;
const NOTICE_WAIT_MS = 400;

/** How long a kind's rows are kept on the page after the person leaves it, like the backend. */
const KEEP_MS = 5 * 60_000;

/** What is kept of a kind, when it is recent; requests that were running are not. */
function recent(kind: FeedKind): FeedData {
  const entry = kept.get(kind);
  if (entry === undefined || Date.now() - entry.at >= KEEP_MS) return empty();
  return { ...entry.data, running: 0 };
}

function empty(): FeedData {
  return {
    generation: null,
    rows: new Map(),
    sources: [],
    running: 0,
    started: false,
    paused: false,
  };
}

/** The rows of each kind, kept while the person looks at a search and comes back. */
const kept = new Map<FeedKind, { data: FeedData; at: number }>();

/** Forgets the rows kept (tests). */
export function forgetLatestFeed(): void {
  kept.clear();
}

let requestCounter = 0;
function nextId(): string {
  requestCounter += 1;
  return `feed-${Date.now().toString(36)}-${requestCounter}`;
}

function upsert(rows: ReadonlyMap<string, FeedRow>, row: FeedRow): Map<string, FeedRow> {
  const next = new Map(rows);
  for (const gone of row.release.absorbs) next.delete(gone);
  next.set(row.release.id, row);
  return next;
}

function setSource(
  sources: readonly FeedSourceView[],
  id: string,
  change: Partial<FeedSourceView>,
): FeedSourceView[] {
  return sources.map((source) => (source.id === id ? { ...source, ...change } : source));
}

/** The state of the feed after one event of a request. Pure, for the tests. */
export function applyFeedEvent(data: FeedData, event: ProviderEvent<FeedItem>): FeedData {
  switch (event.kind) {
    case "started":
      return { ...data, sources: setSource(data.sources, event.provider, { state: "asked" }) };
    case "partial":
    case "results":
    case "update": {
      let rows = data.rows;
      let sources = data.sources;
      for (const item of event.items) {
        if (item.type === "row") rows = upsert(rows, item.row);
        else {
          sources = setSource(sources, event.provider, {
            state: item.state as FeedSourceState,
          });
        }
      }
      return { ...data, rows, sources };
    }
    case "failed":
      return {
        ...data,
        sources: setSource(data.sources, event.provider, { state: "failed", reason: event.reason }),
      };
    case "needsKey":
      return { ...data, sources: setSource(data.sources, event.provider, { state: "failed" }) };
    case "done":
      return data;
  }
}

/** The start reply folded in: sources the request's events already moved keep their state. */
export function applyFeedStart(
  data: FeedData,
  start: FeedStart,
  touched: ReadonlySet<string>,
): FeedData {
  const known = new Map(data.sources.map((source) => [source.id, source]));
  const sources = start.sources.map((source: FeedSource): FeedSourceView => {
    const held = known.get(source.id);
    if (held !== undefined && touched.has(source.id)) return held;
    // A source that failed before and is not asked now keeps saying so.
    if (held?.state === "failed" && source.state === "more") return held;
    return {
      id: source.id,
      name: source.name,
      kind: source.kind,
      state: source.state as FeedSourceState,
    };
  });
  let rows = data.rows;
  for (const row of start.rows) rows = upsert(rows, row);
  return { ...data, generation: start.generation, sources, rows };
}

/**
 * The "Latest releases" feed of the Torrents page: reads the first page of every source when it
 * is enabled (or the kind changes), streams the rows in as each source answers, reads the next
 * page on `loadMore`, and starts over on `refresh`. Each kind keeps its rows for a few minutes,
 * so flipping tabs shows them at once; the backend keeps them too and asks nobody again.
 */
export function useLatestFeed(kind: FeedKind, enabled: boolean): LatestFeed {
  const [data, setData] = useState<FeedData>(() => recent(kind));
  const [shownKind, setShownKind] = useState(kind);
  // A new kind shows what is kept for it at once.
  if (shownKind !== kind) {
    setShownKind(kind);
    setData(recent(kind));
  }
  const running = useRef(new Set<string>());
  const kindNow = useRef(kind);
  const dataNow = useRef(data);
  useEffect(() => {
    kindNow.current = kind;
    dataNow.current = data;
    kept.set(kind, { data, at: kept.get(kind)?.at ?? Date.now() });
  });

  const update = useCallback((forKind: FeedKind, change: (current: FeedData) => FeedData) => {
    if (kindNow.current !== forKind) return;
    setData((current) => {
      const next = change(current);
      dataNow.current = next;
      return next;
    });
  }, []);

  // Set below; a request that finds the feed refreshed elsewhere starts it over.
  const startOver = useRef<(kind: FeedKind) => void>(() => undefined);
  const askAgain = useRef<(kind: FeedKind, more: boolean) => void>(() => undefined);
  const noticeWaits = useRef(0);
  const request = useCallback(
    (forKind: FeedKind, more: boolean, refresh: boolean) => {
      const backend = currentTorrentBackend();
      const searchId = nextId();
      running.current.add(searchId);
      const touched = new Set<string>();
      let finished = false;
      const finish = () => {
        if (finished) return;
        finished = true;
        running.current.delete(searchId);
        update(forKind, (current) => ({ ...current, running: Math.max(0, current.running - 1) }));
      };
      const before = dataNow.current.rows.size;
      update(forKind, (current) => ({
        ...(refresh ? empty() : current),
        running: (refresh ? 0 : current.running) + 1,
        started: true,
        paused: false,
      }));
      if (refresh) kept.set(forKind, { data: empty(), at: Date.now() });
      backend
        .latest(
          {
            searchId,
            kind: forKind,
            more,
            refresh,
            generation: more ? dataNow.current.generation : null,
          },
          (event) => {
            if (!running.current.has(searchId)) return;
            if (event.kind !== "done") touched.add(event.provider);
            update(forKind, (current) => applyFeedEvent(current, event));
            if (event.kind === "done") {
              if (more) {
                update(forKind, (current) => ({
                  ...current,
                  paused:
                    current.rows.size === before &&
                    current.sources.some((source) => source.state === "failed"),
                }));
              }
              finish();
            }
          },
        )
        .then((start) => {
          // A request cancelled meanwhile (a refresh, another kind) says nothing more.
          if (!finished && !running.current.has(searchId)) return;
          update(forKind, (current) => applyFeedStart(current, start, touched));
        })
        .catch((error: unknown) => {
          finish();
          if (!(error instanceof TorrentRequestError)) return;
          if (error.code === "feed_expired") {
            // Another window refreshed the feed: start over so the pages line up again.
            startOver.current(forKind);
          } else if (error.code === "notice_not_accepted" && noticeWaits.current < NOTICE_WAITS) {
            // "Got it" was just pressed and the setting is still being saved: ask again shortly.
            noticeWaits.current += 1;
            window.setTimeout(() => askAgain.current(forKind, more), NOTICE_WAIT_MS);
          }
        });
      return searchId;
    },
    [update],
  );

  useEffect(() => {
    startOver.current = (forKind) => request(forKind, false, true);
    askAgain.current = (forKind, more) => request(forKind, more, false);
  }, [request]);

  // The first page: when the feed is shown, and again for each kind.
  useEffect(() => {
    if (!enabled) return;
    const ids = running.current;
    request(kind, false, false);
    return () => {
      const backend = currentTorrentBackend();
      for (const id of ids) backend.cancelLatest(id);
      ids.clear();
      // What was running is no longer; the next first-page request picks the sources up again.
      setData((current) => ({ ...current, running: 0 }));
    };
  }, [kind, enabled, request]);

  const loadMore = useCallback(() => {
    if (dataNow.current.running > 0) return;
    request(kindNow.current, true, false);
  }, [request]);

  const refresh = useCallback(() => {
    const backend = currentTorrentBackend();
    for (const id of running.current) backend.cancelLatest(id);
    running.current.clear();
    request(kindNow.current, false, true);
  }, [request]);

  const rows = useMemo(() => [...data.rows.values()], [data.rows]);
  const hasMore = data.sources.some(
    (source) => source.state === "more" || source.state === "failed",
  );
  return {
    rows,
    sources: data.sources,
    loading: data.running > 0,
    firstLoad: rows.length === 0 && (data.running > 0 || (enabled && !data.started)),
    hasMore,
    paused: data.paused,
    loadMore,
    refresh,
  };
}
