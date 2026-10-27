import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { useJobProgressStore } from "../jobs/jobProgressStore";
import { newlyFinishedMatchJobs } from "./matchJobs";
import { currentReviewBackend } from "./reviewBackend";
import { REVIEW_COUNT_KEY, REVIEW_QUEUE_KEY } from "./useScrapeQueue";

/** Queries of pages that show scraped data, read again when a match job finishes. */
const REFRESHED_BY_MATCH: readonly (readonly string[])[] = [
  ["library"],
  ["movie-detail"],
  ["tv-show"],
];

/**
 * Reads the Scrape page's review rows and their count again whenever a background scrape job
 * finishes (its uncertain titles have just arrived). Mount it once, in the shell. The sidebar's
 * Scrape number comes from the pipeline counts, not from here.
 */
export function useRefreshQueueAfterMatch(): void {
  const backend = currentReviewBackend();
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!backend.available) return undefined;
    return useJobProgressStore.subscribe((state, previous) => {
      if (state.jobs === previous.jobs) return;
      if (newlyFinishedMatchJobs(previous.jobs, state.jobs).length === 0) return;
      void queryClient.invalidateQueries({ queryKey: REVIEW_COUNT_KEY });
      void queryClient.invalidateQueries({ queryKey: REVIEW_QUEUE_KEY });
      // The sidebar's Scrape number is the pipeline count.
      void queryClient.invalidateQueries({ queryKey: ["lifecycle"] });
      // The batch rewrote titles: pages that show them read them again.
      for (const key of REFRESHED_BY_MATCH) void queryClient.invalidateQueries({ queryKey: key });
    });
  }, [backend.available, queryClient]);
}
