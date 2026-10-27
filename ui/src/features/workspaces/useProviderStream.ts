import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { KEY_REJECTED, UNAVAILABLE } from "./providerEvents";
import type {
  ProviderEvent,
  ProviderItem,
  ProviderSearchTransport,
  ProviderStatus,
} from "./providerEvents";

export interface ProviderStreamOptions<T> {
  transport: ProviderSearchTransport<T>;
  /** The text to search for. A new value cancels the running search and starts another. */
  query: string;
  params?: Readonly<Record<string, string | number | boolean>>;
  /** False holds the search back (a workspace that is not open, a query that is too short). */
  enabled?: boolean;
}

export interface ProviderStream<T> {
  /** Every result received so far, in arrival order, each with its provider. */
  items: readonly ProviderItem<T>[];
  /** One status per provider that has said anything, in the order they first did. */
  providers: readonly ProviderStatus[];
  /** True until the backend says every provider has finished. */
  searching: boolean;
  /** Asks one provider again; the others are left alone. */
  retry: (provider: string) => void;
}

interface StreamState<T> {
  items: readonly ProviderItem<T>[];
  providers: readonly ProviderStatus[];
  done: boolean;
}

function empty<T>(): StreamState<T> {
  return { items: [], providers: [], done: false };
}

let searchCounter = 0;
function nextSearchId(): string {
  searchCounter += 1;
  return `search-${Date.now().toString(36)}-${searchCounter}`;
}

function upsert(
  providers: readonly ProviderStatus[],
  provider: string,
  change: (current: ProviderStatus) => ProviderStatus,
): ProviderStatus[] {
  const index = providers.findIndex((entry) => entry.provider === provider);
  if (index === -1) {
    return [...providers, change({ provider, state: "searching", count: 0, retryable: false })];
  }
  return providers.map((entry, i) => (i === index ? change(entry) : entry));
}

/** The state after one event; pure, so the reducer is easy to test and to reason about. */
export function applyProviderEvent<T>(
  state: StreamState<T>,
  event: ProviderEvent<T>,
): StreamState<T> {
  switch (event.kind) {
    case "started":
      return {
        ...state,
        providers: upsert(state.providers, event.provider, (current) => ({
          provider: current.provider,
          state: "searching",
          count: current.count,
          retryable: false,
        })),
      };
    case "results":
      return {
        ...state,
        items: [...state.items, ...event.items.map((item) => ({ provider: event.provider, item }))],
        providers: upsert(state.providers, event.provider, (current) => ({
          ...current,
          // A provider's answer, even an empty one, ends its searching; later batches add to it.
          state: "done",
          count: current.count + event.items.length,
        })),
      };
    case "partial":
      // More of an answer that is still coming: the provider stays "searching".
      return {
        ...state,
        items: [...state.items, ...event.items.map((item) => ({ provider: event.provider, item }))],
        providers: upsert(state.providers, event.provider, (current) => ({
          ...current,
          state: "searching",
          count: current.count + event.items.length,
        })),
      };
    case "update":
      // Items sent before, improved: a flow that keys its items replaces the old ones; the count
      // and the provider's state stay as they are.
      return {
        ...state,
        items: [...state.items, ...event.items.map((item) => ({ provider: event.provider, item }))],
      };
    case "failed":
      return {
        ...state,
        providers: upsert(state.providers, event.provider, (current) => ({
          provider: current.provider,
          state:
            event.reason === KEY_REJECTED
              ? "rejected"
              : event.reason === UNAVAILABLE
                ? "unavailable"
                : "failed",
          count: current.count,
          reason: event.reason,
          retryable: event.retryable,
        })),
      };
    case "needsKey":
      return {
        ...state,
        providers: upsert(state.providers, event.provider, (current) => ({
          provider: current.provider,
          state: "needs-key",
          count: current.count,
          retryable: false,
        })),
      };
    case "done":
      // Every provider that was still searching has finished.
      return {
        ...state,
        done: true,
        providers: state.providers.map((entry) =>
          entry.state === "searching" ? { ...entry, state: "done" } : entry,
        ),
      };
  }
}

/**
 * Subscribes to one provider search by its `search_id`: per-provider status, results
 * appended as they arrive and no waiting for the slowest provider. A provider that fails or
 * needs a key never hides the others. Changing the query cancels the old search and starts a
 * new one; unmounting cancels the running search, so no provider work leaks.
 */
export function useProviderStream<T>({
  transport,
  query,
  params,
  enabled = true,
}: ProviderStreamOptions<T>): ProviderStream<T> {
  // The parameters are an object; compare them by value so a new literal does not restart.
  const paramsKey = JSON.stringify(params ?? {});
  // What a search is for. The state remembers the key it belongs to, so a new key shows an empty
  // stream at once and no effect has to reset anything.
  const key = `${query}\0${paramsKey}`;
  const [kept, setKept] = useState<{ key: string; stream: StreamState<T> }>(() => ({
    key,
    stream: empty<T>(),
  }));
  const state = kept.key === key ? kept.stream : empty<T>();
  const searchId = useRef<string | null>(null);
  const transportNow = useRef(transport);
  useEffect(() => {
    transportNow.current = transport;
  });

  useEffect(() => {
    if (!enabled) return;
    const id = nextSearchId();
    searchId.current = id;
    const active = transportNow.current;
    active.start(
      {
        searchId: id,
        query,
        ...(params === undefined ? {} : { params }),
      },
      (event) => {
        // Events of a search that was replaced or cancelled are ignored.
        if (searchId.current !== id) return;
        setKept((current) => ({
          key,
          stream: applyProviderEvent(current.key === key ? current.stream : empty<T>(), event),
        }));
      },
    );
    return () => {
      if (searchId.current === id) searchId.current = null;
      active.cancel(id);
    };
    // `params` is covered by its serialised form inside `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled]);

  const retry = useCallback(
    (provider: string) => {
      const id = searchId.current;
      if (id === null) return;
      // Back to searching at once, so the chip answers the click before the backend does.
      setKept((current) => ({
        key,
        stream: {
          ...(current.key === key ? current.stream : empty<T>()),
          providers: (current.key === key ? current.stream : empty<T>()).providers.map((entry) =>
            entry.provider === provider
              ? {
                  provider: entry.provider,
                  state: "searching" as const,
                  count: entry.count,
                  retryable: false,
                }
              : entry,
          ),
        },
      }));
      transportNow.current.retry(id, provider);
    },
    [key],
  );

  return useMemo(
    () => ({
      items: state.items,
      providers: state.providers,
      // Searching until the stream is done, and again while a retried provider has not answered.
      searching:
        enabled && (!state.done || state.providers.some((entry) => entry.state === "searching")),
      retry,
    }),
    [state, enabled, retry],
  );
}
