import type { QueryClient } from "@tanstack/react-query";

import type { MovieDetail } from "../detail/detailBackend";
import type { ScrapeStreamHit } from "./scrapeBackend";

/**
 * Shows a chosen match on the movie page before the backend has answered: the title, year, plot
 * and runtime the entry carries. Returns the function that puts the page's old values back, for
 * a refusal. A page that is not cached is left alone.
 */
export function patchMovieDetail(
  queryClient: QueryClient,
  itemId: string,
  hit: ScrapeStreamHit,
): () => void {
  const key = ["movie-detail", itemId] as const;
  const before = queryClient.getQueryData<MovieDetail>(key);
  if (before === undefined) return () => undefined;
  const { candidate } = hit;
  queryClient.setQueryData<MovieDetail>(key, {
    ...before,
    title: candidate.title,
    year: candidate.year ?? before.year,
    plot: candidate.overview ?? before.plot,
    runtime_minutes: candidate.runtimeMinutes ?? before.runtime_minutes,
  });
  return () => queryClient.setQueryData<MovieDetail>(key, before);
}
