import { useCallback, useMemo, useState } from "react";

import { useProviderStream } from "../workspaces/useProviderStream";
import type { ProviderStatus } from "../workspaces/providerEvents";
import { currentTorrentBackend } from "./torrentBackend";
import type {
  ReleaseSearchTarget,
  SearchStart,
  SourceInfo,
  TorrentRelease,
} from "./torrentBackend";
import { mergeReleases } from "./torrentModel";

export interface ReleaseSearch {
  /** Every source of the search, with the reason when it is skipped; empty until the start reply. */
  sources: readonly SourceInfo[];
  /** The merged releases so far. */
  releases: readonly TorrentRelease[];
  /** One status per source that was asked. */
  providers: readonly ProviderStatus[];
  /** True until every source has answered. */
  searching: boolean;
  /** Asks one source again. */
  retry: (source: string) => void;
  /** Asks every failed source again. */
  retryAll: () => void;
  /** Starts the whole search again ("Search again"). */
  restart: () => void;
}

/** The request parameters of a target. */
function paramsOf(target: ReleaseSearchTarget, attempt: number): Record<string, string | number> {
  return {
    attempt,
    kind: target.kind,
    tmdbId: target.tmdbId,
    ...(target.season === undefined ? {} : { season: target.season }),
    ...(target.episode === undefined ? {} : { episode: target.episode }),
  };
}

/**
 * The release search of a confirmed title and scope: starts when there is a target, restarts
 * when the title or the scope changes (the old search is cancelled), and cancels when the page
 * goes away. The releases come merged by the backend; a row sent again replaces the one held.
 */
export function useReleaseSearch(target: ReleaseSearchTarget | null): ReleaseSearch {
  const [attempt, setAttempt] = useState(0);
  const params = useMemo(
    () => (target === null ? undefined : paramsOf(target, attempt)),
    [target, attempt],
  );
  const key = params === undefined ? "" : JSON.stringify(params);
  const [started, setStarted] = useState<{ key: string; sources: readonly SourceInfo[] }>({
    key: "",
    sources: [],
  });
  // One transport per search, so its start reply is filed under the search it belongs to.
  const transport = useMemo(
    () =>
      currentTorrentBackend().releaseTransport((start: SearchStart) =>
        setStarted({ key, sources: start.sources }),
      ),
    [key],
  );
  const stream = useProviderStream<TorrentRelease>({
    transport,
    query: "",
    ...(params === undefined ? {} : { params }),
    enabled: target !== null,
  });
  const releases = useMemo(() => mergeReleases(stream.items), [stream.items]);
  const { providers, retry: retryOne } = stream;
  const retryAll = useCallback(() => {
    for (const status of providers) {
      if (status.state === "failed" || status.state === "rejected") retryOne(status.provider);
    }
  }, [providers, retryOne]);
  const restart = useCallback(() => setAttempt((value) => value + 1), []);
  return {
    sources: started.key === key ? started.sources : [],
    releases,
    providers,
    searching: stream.searching,
    retry: retryOne,
    retryAll,
    restart,
  };
}
