import { useCallback, useEffect, useSyncExternalStore } from "react";

import { currentTorrentBackend } from "./torrentBackend";
import type { OwnJackett } from "./torrentBackend";

/** What the last look found, shared by the Settings card and the Torrent search page. */
let last: OwnJackett | null | undefined;
let pending: Promise<OwnJackett | null> | null = null;
const listeners = new Set<() => void>();

function look(): Promise<OwnJackett | null> {
  pending ??= currentTorrentBackend()
    .detectJackett()
    .catch(() => null)
    .then((found) => {
      last = found;
      pending = null;
      for (const listener of listeners) listener();
      return found;
    });
  return pending;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

const snapshot = () => last;

/** Forgets what was found, so the next reader looks again. Used by tests. */
export function forgetOwnJackett(): void {
  last = undefined;
  pending = null;
}

/**
 * A Jackett the person runs themselves: looked for once per session (it only asks its feed, no
 * key), and again after `refresh` (when the own-instance setting changed). `undefined` while the
 * first look runs, `null` when none answers.
 */
export function useOwnJackett(): {
  found: OwnJackett | null | undefined;
  refresh: () => Promise<OwnJackett | null>;
} {
  const found = useSyncExternalStore(subscribe, snapshot, snapshot);
  useEffect(() => {
    if (last === undefined) void look();
  }, []);
  const refresh = useCallback(() => {
    pending = null;
    return look();
  }, []);
  return { found, refresh };
}
