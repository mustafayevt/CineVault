import type { LifecycleCounts } from "../lifecycle/lifecycleBackend";
import { useLifecycleCounts } from "../lifecycle/useLifecycleCounts";

export interface NavCounts {
  /** Titles waiting on the Scrape page: not scraped yet, uncertain and not matched. */
  scrape: number;
  /** Titles whose names differ from the naming templates. */
  rename: number;
}

/** The waiting counts of one library type (Scrape = New + Uncertain + Not matched). */
function navCountsOf(counts: LifecycleCounts | undefined): NavCounts {
  if (counts === undefined) return { scrape: 0, rename: 0 };
  return {
    scrape: counts.not_scraped + counts.uncertain + counts.not_matched,
    rename: counts.rename_pending,
  };
}

/** The Scrape and Rename counts of the sidebar, movies and shows together. */
export function useNavCounts(): NavCounts {
  const movies = navCountsOf(useLifecycleCounts("movies").data);
  const shows = navCountsOf(useLifecycleCounts("shows").data);
  return { scrape: movies.scrape + shows.scrape, rename: movies.rename + shows.rename };
}
