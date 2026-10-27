import { useQuery } from "@tanstack/react-query";
import { currentTorrentBackend } from "./torrentBackend";
import { titleQueryKey } from "./torrentModel";
import type { TitleRef } from "./torrentsSearch";

/** How often a header whose pictures were still downloading asks again, and how many times. */
const ART_RETRY_MS = 2500;
const ART_RETRIES = 4;

/** The details of a chosen title, asked again while its poster or backdrop is downloading. */
export function useTitleDetails(title: TitleRef, enabled = true) {
  return useQuery({
    enabled,
    queryKey: titleQueryKey(title),
    queryFn: () => currentTorrentBackend().details(title.kind, title.tmdbId),
    retry: false,
    staleTime: 5 * 60_000,
    refetchInterval: (query) => {
      const data = query.state.data;
      if (data === undefined || query.state.dataUpdateCount > ART_RETRIES) return false;
      return data.posterKey === null || data.backdropKey === null ? ART_RETRY_MS : false;
    },
  });
}
