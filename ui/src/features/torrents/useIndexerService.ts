import { useCallback, useEffect, useRef, useState } from "react";

import { showToast } from "../toasts/toastStore";
import { currentTorrentBackend } from "./torrentBackend";
import type { IndexerInfo, IndexerManagerAccess, IndexerServiceStatus } from "./torrentBackend";

/** How often the card reads the service while nothing is going on. */
const IDLE_MS = 10_000;
/** How often while it downloads, starts or checks, so the card follows along. */
const BUSY_MS = 1_500;

/** Whether the service is in the middle of something. */
function isBusy(status: IndexerServiceStatus | null): boolean {
  return (
    status !== null &&
    (status.state === "downloading" || status.state === "starting" || status.checking)
  );
}

interface IndexerService {
  /** `null` until the first answer. */
  status: IndexerServiceStatus | null;
  indexers: readonly IndexerInfo[];
  /** A command is under way. */
  working: boolean;
  /** Reads the service and its indexers now. */
  refresh: () => Promise<void>;
  download: () => Promise<void>;
  update: () => Promise<void>;
  /** Checks the indexers now. */
  check: () => Promise<void>;
  /** Opens the service's own page; resolves with the admin password to copy. */
  open: () => Promise<IndexerManagerAccess | null>;
  setIndexer: (id: string, on: boolean) => Promise<void>;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The managed indexer service for the Torrent sources page: its state and indexers, read when the
 * page opens, every ten seconds, and every second and a half while it downloads, starts or
 * checks; and the commands of its card. A failed command is a toast with the reason in words.
 */
export function useIndexerService(): IndexerService {
  const [status, setStatus] = useState<IndexerServiceStatus | null>(null);
  const [indexers, setIndexers] = useState<readonly IndexerInfo[]>([]);
  const [working, setWorking] = useState(false);
  const alive = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const backend = currentTorrentBackend();
      const [next, rows] = await Promise.all([backend.indexerStatus(), backend.indexers()]);
      if (!alive.current) return;
      setStatus(next);
      setIndexers(rows);
    } catch {
      // The card keeps what it had; the next read tries again.
    }
  }, []);

  const busy = isBusy(status);
  useEffect(() => {
    alive.current = true;
    const first = window.setTimeout(() => {
      void refresh();
    }, 0);
    const timer = window.setInterval(
      () => {
        void refresh();
      },
      busy ? BUSY_MS : IDLE_MS,
    );
    return () => {
      alive.current = false;
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [refresh, busy]);

  const run = useCallback(
    async <T>(work: () => Promise<T>): Promise<T | null> => {
      setWorking(true);
      try {
        return await work();
      } catch (error) {
        showToast({ tone: "error", message: messageOf(error) });
        return null;
      } finally {
        if (alive.current) setWorking(false);
        await refresh();
      }
    },
    [refresh],
  );

  return {
    status,
    indexers,
    working,
    refresh,
    download: async () => {
      await run(() => currentTorrentBackend().installIndexer());
    },
    update: async () => {
      await run(() => currentTorrentBackend().updateIndexer());
    },
    check: async () => {
      await run(() => currentTorrentBackend().checkSources());
    },
    open: () => run(() => currentTorrentBackend().openIndexerManager()),
    setIndexer: async (id, on) => {
      await run(() => currentTorrentBackend().setIndexer(id, on));
    },
  };
}
