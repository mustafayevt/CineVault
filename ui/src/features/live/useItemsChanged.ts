import { isTauri } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { useEffect } from "react";

import { events } from "../../bindings";
import type { ItemsChanged } from "../../bindings";
import { useHighlightStore } from "./highlightStore";
import { itemKeys, listKeys } from "./itemQueries";
import type { ChangePart } from "./itemQueries";

/** Where changes come from: the desktop event in the app, a fake in tests. Returns how to stop. */
export type ItemsChangedSource = (handler: (change: ItemsChanged) => void) => Promise<() => void>;

const tauriSource: ItemsChangedSource = async (handler) =>
  events.itemsChanged.listen((event) => handler(event.payload));

let override: ItemsChangedSource | null = null;

/** Replaces the source; pass `null` to go back to the desktop event. Used by tests. */
export function setItemsChangedSource(source: ItemsChangedSource | null): void {
  override = source;
}

/** Events that come within this time are applied together. */
export const COLLECT_MS = 120;
/** Lists and counts refresh at most this often, however many events come. */
const LIST_MIN_MS = 1000;

function partsOf(changes: readonly ItemsChanged[]): Set<ChangePart> {
  const parts = new Set<ChangePart>();
  for (const change of changes) for (const part of change.parts) parts.add(part);
  return parts;
}

/** Invalidates each distinct key once, however many changes named it. */
function invalidateOnce(client: QueryClient, keys: readonly QueryKey[]): void {
  const distinct = new Map<string, QueryKey>();
  for (const queryKey of keys) distinct.set(JSON.stringify(queryKey), queryKey);
  for (const queryKey of distinct.values()) void client.invalidateQueries({ queryKey });
}

function invalidateLists(client: QueryClient, changes: readonly ItemsChanged[]): void {
  const parts = partsOf(changes);
  if (parts.size > 0) invalidateOnce(client, listKeys(parts));
}

/**
 * Applies what changed: invalidates the queries of each changed part of each title, refreshes the
 * lists and counts they feed (unless `lists` is false), and highlights the changed titles. Only
 * the parts that changed are touched, so a ratings event leaves the files and the artwork
 * queries alone.
 */
function applyItemsChanged(
  client: QueryClient,
  changes: readonly ItemsChanged[],
  options: { lists?: boolean } = {},
): void {
  const titles = new Set<string>();
  const keys: QueryKey[] = [];
  for (const change of changes) {
    for (const part of change.parts) {
      for (const id of change.items) keys.push(...itemKeys(id, part));
    }
    for (const id of change.items) titles.add(id);
  }
  invalidateOnce(client, keys);
  if (options.lists ?? true) invalidateLists(client, changes);
  useHighlightStore.getState().mark([...titles]);
}

/**
 * Keeps the open pages in step with the library: when a write changes a title (a rating arrives,
 * a poster is placed, a rename is applied), the queries of what changed refresh and the changed
 * titles flash. Mount it once, in the shell. A burst of events is applied together, and lists
 * refresh at most once a second.
 */
export function useItemsChangedSync(): void {
  const client = useQueryClient();
  useEffect(() => {
    let source = override;
    if (source === null && isTauri()) source = tauriSource;
    if (source === null) return undefined;

    let queue: ItemsChanged[] = [];
    let timer: ReturnType<typeof setTimeout> | undefined;
    let lastLists = Number.NEGATIVE_INFINITY;
    let waitingLists: ItemsChanged[] = [];
    let listTimer: ReturnType<typeof setTimeout> | undefined;

    const flushLists = () => {
      listTimer = undefined;
      const batch = waitingLists;
      waitingLists = [];
      lastLists = Date.now();
      invalidateLists(client, batch);
    };
    const flush = () => {
      timer = undefined;
      const batch = queue;
      queue = [];
      // The titles' own queries now; the lists and counts no more than once per LIST_MIN_MS.
      applyItemsChanged(client, batch, { lists: false });
      waitingLists.push(...batch);
      const wait = lastLists + LIST_MIN_MS - Date.now();
      if (wait <= 0) flushLists();
      else listTimer ??= setTimeout(flushLists, wait);
    };

    let active = true;
    let stop: (() => void) | undefined;
    source((change) => {
      queue.push(change);
      timer ??= setTimeout(flush, COLLECT_MS);
    })
      .then((unlisten) => {
        if (active) stop = unlisten;
        else unlisten();
      })
      .catch(() => {
        // Not inside the desktop shell: no events arrive.
      });
    return () => {
      active = false;
      stop?.();
      if (timer !== undefined) clearTimeout(timer);
      if (listTimer !== undefined) clearTimeout(listTimer);
    };
  }, [client]);
}
