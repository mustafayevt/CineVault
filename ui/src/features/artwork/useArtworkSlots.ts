import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  ProviderItem,
  ProviderSearchTransport,
  ProviderStatus,
} from "../workspaces/providerEvents";
import { applyProviderEvent } from "../workspaces/useProviderStream";
import { currentArtworkBackend } from "./artworkBackend";
import type { ArtworkCandidate } from "./artworkBackend";
import { stripSlots } from "./pickerModel";

/** How many types are searched at once; the type on screen always goes first. */
const SEARCH_AT_ONCE = 2;

type Raw = Parameters<typeof applyProviderEvent<ArtworkCandidate>>[0];

/** What is known of one artwork type while the pickers search it. */
export interface SlotState {
  /** Every image offered, once each, in the order the providers answered. */
  items: readonly ArtworkCandidate[];
  providers: readonly ProviderStatus[];
  /** Providers are still answering. */
  searching: boolean;
  /** The image the title has now for this type, when it has one. */
  current: ArtworkCandidate | null;
  /**
   * How many images the providers offer, from the counts-only lookup. It stands for the type
   * until its own search has answered; `undefined` when the type was not counted.
   */
  count?: number;
}

interface Entry {
  raw: Raw;
  current: ArtworkCandidate | null;
}

const EMPTY: Entry = { raw: { items: [], providers: [], done: false }, current: null };

/** What the counts-only lookup says of the types: `undefined` while it runs, `null` when unsupported. */
type Counted = Readonly<Record<string, { count: number; current: ArtworkCandidate | null }>>;

function unique(items: readonly ProviderItem<ArtworkCandidate>[]): ArtworkCandidate[] {
  const seen = new Set<string>();
  const list: ArtworkCandidate[] = [];
  for (const { item } of items) {
    if (seen.has(item.id)) continue;
    seen.add(item.id);
    list.push(item);
  }
  return list;
}

let counter = 0;

/**
 * The searches of the artwork picker (spec `artwork` "Artwork picker page"): the type on screen is
 * searched at once, and the other types of the title one after another in the background, so the
 * type strip can show how many images each has and a type opens with its images already there.
 * Each type is one streaming search of the artwork candidate stream; nothing is written. Leaving
 * the page stops the searches.
 */
export function useArtworkSlots(itemId: string, active: string, only?: readonly string[]) {
  const [entries, setEntries] = useState<Readonly<Record<string, Entry>>>({});
  const [meta, setMeta] = useState<{
    title: string;
    year: number | null;
    slots: readonly string[];
    names: Readonly<Record<string, string>>;
  } | null>(null);
  const [counted, setCounted] = useState<Counted | null | undefined>(undefined);
  const started = useRef(new Set<string>());
  const running = useRef(
    new Map<string, { transport: ProviderSearchTransport<ArtworkCandidate>; searchId: string }>(),
  );

  const begin = useCallback(
    (slot: string) => {
      if (started.current.has(slot)) return;
      started.current.add(slot);
      counter += 1;
      const searchId = `art-${Date.now().toString(36)}-${counter}`;
      const transport = currentArtworkBackend().searchTransport((start) => {
        setMeta({
          title: start.title,
          year: start.year,
          slots: start.slots,
          names: Object.fromEntries(start.providers.map((entry) => [entry.key, entry.name])),
        });
        setEntries((previous) => ({
          ...previous,
          [slot]: { raw: (previous[slot] ?? EMPTY).raw, current: start.current },
        }));
      });
      running.current.set(slot, { transport, searchId });
      setEntries((previous) => ({ ...previous, [slot]: previous[slot] ?? EMPTY }));
      transport.start({ searchId, query: slot, params: { itemId, slot } }, (event) => {
        setEntries((previous) => {
          const entry = previous[slot] ?? EMPTY;
          return { ...previous, [slot]: { ...entry, raw: applyProviderEvent(entry.raw, event) } };
        });
      });
    },
    [itemId],
  );

  // The type strip's counts and current images come from one counts-only lookup; only the type on
  // screen is searched. An item the lookup does not count (a season, an episode) searches each type.
  useEffect(() => {
    let live = true;
    currentArtworkBackend()
      .counts(itemId)
      .then(
        (found) => {
          if (!live) return;
          setCounted(
            found === null
              ? null
              : Object.fromEntries(
                  found.map((entry) => [
                    entry.slot,
                    { count: entry.count, current: entry.current },
                  ]),
                ),
          );
        },
        () => live && setCounted(null),
      );
    return () => {
      live = false;
    };
  }, [itemId]);

  const slots = useMemo(() => {
    if (counted !== null && counted !== undefined) {
      const listed = stripSlots(Object.keys(counted));
      if (listed.length > 0)
        return only === undefined ? listed : listed.filter((s) => only.includes(s));
    }
    if (meta === null) return [active];
    const all = stripSlots(meta.slots);
    return only === undefined ? all : all.filter((slot) => only.includes(slot));
  }, [counted, meta, active, only]);
  const busy = Object.values(entries).filter((entry) => !entry.raw.done).length;

  // The type on screen first, then the others in the order of the strip.
  useEffect(() => {
    begin(active);
    // Waiting for the counts: they may spare the other searches. Counted: nothing more to search.
    if (meta === null || counted === undefined || counted !== null) return;
    let room = SEARCH_AT_ONCE - busy;
    for (const slot of slots) {
      if (room <= 0) break;
      if (started.current.has(slot)) continue;
      begin(slot);
      room -= 1;
    }
  }, [active, begin, busy, counted, meta, slots]);

  useEffect(() => {
    const live = running.current;
    const begun = started.current;
    return () => {
      for (const { transport, searchId } of live.values()) transport.cancel(searchId);
      live.clear();
      // A remount (React's development double run) searches again what was cancelled.
      begun.clear();
    };
  }, []);

  const retry = useCallback((slot: string, provider: string) => {
    const search = running.current.get(slot);
    if (search === undefined) return;
    setEntries((previous) => {
      const entry = previous[slot];
      if (entry === undefined) return previous;
      return {
        ...previous,
        [slot]: {
          ...entry,
          raw: {
            ...entry.raw,
            providers: entry.raw.providers.map((status) =>
              status.provider === provider
                ? { ...status, state: "searching" as const, retryable: false }
                : status,
            ),
          },
        },
      };
    });
    search.transport.retry(search.searchId, provider);
  }, []);

  /** Searches a type again, after a file or a link replaced its image. */
  const reload = useCallback(
    (slot: string) => {
      const search = running.current.get(slot);
      if (search !== undefined) search.transport.cancel(search.searchId);
      running.current.delete(slot);
      started.current.delete(slot);
      setEntries((previous) => ({ ...previous, [slot]: EMPTY }));
      begin(slot);
    },
    [begin],
  );

  const states = useMemo(() => {
    const out: Record<string, SlotState> = {};
    // A counted type that is not open yet: its count and current image, and searching, so that
    // opening it shows the wait and not an empty list before its search starts.
    for (const [slot, known] of Object.entries(counted ?? {})) {
      out[slot] = {
        items: [],
        providers: [],
        searching: true,
        current: known.current,
        count: known.count,
      };
    }
    for (const [slot, entry] of Object.entries(entries)) {
      out[slot] = {
        items: unique(entry.raw.items),
        providers: entry.raw.providers,
        searching:
          !entry.raw.done || entry.raw.providers.some((status) => status.state === "searching"),
        current: entry.current ?? counted?.[slot]?.current ?? null,
        ...(counted?.[slot] === undefined ? {} : { count: counted[slot].count }),
      };
    }
    return out;
  }, [entries, counted]);

  return {
    /** The title, once a search has told it. */
    title: meta?.title ?? null,
    year: meta?.year ?? null,
    /** The types the strip lists. */
    slots,
    /** Provider names by key, once a search has told them. */
    names: meta?.names ?? {},
    states,
    retry,
    reload,
  };
}
