import { currentDetailBackend } from "../detail/detailBackend";
import type { DetailCredit } from "../detail/detailBackend";
import { imdbPersonId, imdbPersonUrl, tmdbPersonId, tmdbPersonUrl } from "../detail/castModel";
import { openRatingPage } from "../ratings/ratingPage";

/** Whether a person has anything to open: an IMDb id or a TMDB id. */
export function hasPersonPage(person: DetailCredit): boolean {
  return imdbPersonId(person) !== null || tmdbPersonId(person) !== null;
}

/**
 * Opens a person's IMDb page in the default browser. The stored IMDb id is used when there is one;
 * otherwise it is asked of TMDB by the person's TMDB id (the backend stores the answer). When
 * IMDb has nothing, or cannot be asked, the person's TMDB page opens instead. A page that cannot
 * be opened says so in a toast.
 */
export async function openPersonPage(person: DetailCredit): Promise<void> {
  const stored = imdbPersonId(person);
  if (stored !== null) return openRatingPage(imdbPersonUrl(stored), "IMDb");
  const tmdb = tmdbPersonId(person);
  if (tmdb === null) return;
  const asked = await currentDetailBackend()
    .personImdbId(tmdb)
    .catch(() => null);
  if (asked !== null && /^nm\d+$/.test(asked)) {
    return openRatingPage(imdbPersonUrl(asked), "IMDb");
  }
  return openRatingPage(tmdbPersonUrl(tmdb), "TMDB");
}
