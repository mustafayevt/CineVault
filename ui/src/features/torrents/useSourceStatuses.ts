import { useCallback, useEffect, useRef, useState } from "react";

import { useSettingsStore } from "../settings/settingsStore";
import { showToast } from "../toasts/toastStore";
import { currentTorrentBackend } from "./torrentBackend";
import type { SourceStatus } from "./torrentBackend";

/** The setting that holds a source's switch. */
function sourceSettingId(id: string): string {
  return id === "indexer-service" ? "torrents.indexer-service.enabled" : `torrents.source.${id}`;
}

interface SourceStatuses {
  /** `null` until the first answer. */
  data: readonly SourceStatus[] | null;
  /** A read is under way. */
  fetching: boolean;
  /** Reads the sources again now. */
  refetch: () => Promise<void>;
  /** Switches a source and takes the answer in. */
  setSource: (id: string, on: boolean) => Promise<void>;
  /** A switch is being saved. */
  saving: boolean;
}

/**
 * The state of the sources for the Settings page, read when it opens and every `everyMs` while it
 * stays open. Plain state rather than a query so the page needs no query client of its own.
 */
export function useSourceStatuses(everyMs = 10_000): SourceStatuses {
  const [data, setData] = useState<readonly SourceStatus[] | null>(null);
  const [fetching, setFetching] = useState(false);
  const [saving, setSaving] = useState(false);
  const alive = useRef(true);

  const refetch = useCallback(async () => {
    setFetching(true);
    try {
      const answer = await currentTorrentBackend().sourcesStatus();
      if (alive.current) setData(answer);
    } catch {
      // The list keeps what it had; the next read tries again.
    } finally {
      if (alive.current) setFetching(false);
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    const first = window.setTimeout(() => {
      void refetch();
    }, 0);
    const timer = window.setInterval(() => {
      void refetch();
    }, everyMs);
    return () => {
      alive.current = false;
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [refetch, everyMs]);

  const setSource = useCallback(async (id: string, on: boolean) => {
    setSaving(true);
    try {
      const answer = await currentTorrentBackend().setSource(id, on);
      if (alive.current) setData(answer);
      // The command wrote the setting; the page keeps its own copy in step.
      useSettingsStore.getState().set(sourceSettingId(id), on);
    } catch (error) {
      showToast({
        tone: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      if (alive.current) setSaving(false);
    }
  }, []);

  return { data, fetching, refetch, setSource, saving };
}
