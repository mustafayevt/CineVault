import type { LifecycleCounts } from "../lifecycle/lifecycleBackend";
import { useLifecycleCounts } from "../lifecycle/useLifecycleCounts";
import { useWishlistNewCount } from "../torrents/wishlistQueries";

export interface NavCounts {
  /** Titles waiting on the Scrape page: not scraped yet, uncertain and not matched. */
  scrape: number;
  /** Titles whose names differ from the naming templates. */
  rename: number;
  /** New Wishlist matches nobody has looked at yet. */
  torrents: number;
}

/** The waiting counts of one library type (Scrape = New + Uncertain + Not matched). */
function navCountsOf(counts: LifecycleCounts | undefined): Omit<NavCounts, "torrents"> {
  if (counts === undefined) return { scrape: 0, rename: 0 };
  return {
    scrape: counts.not_scraped + counts.uncertain + counts.not_matched,
    rename: counts.rename_pending,
  };
}

/** The Scrape and Rename counts of the sidebar (movies and shows together) and the new Wishlist matches. */
export function useNavCounts(): NavCounts {
  const movies = navCountsOf(useLifecycleCounts("movies").data);
  const shows = navCountsOf(useLifecycleCounts("shows").data);
  const torrents = useWishlistNewCount();
  return {
    scrape: movies.scrape + shows.scrape,
    rename: movies.rename + shows.rename,
    torrents,
  };
}
