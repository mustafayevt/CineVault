import type { QueryClient, QueryKey } from "@tanstack/react-query";

import type { DetailArtworkDto, TvSeasonDto } from "../../bindings";
import type { ArtworkCandidate, ArtworkImage } from "./artworkBackend";

/**
 * A candidate's image as the page shows it before the file is written: marked, so the hero loads
 * it from a URL of its own and loads the written file again once the page is read anew (same key,
 * so the URL would not change otherwise, and the small stand-in would stay).
 */
function previewImage(image: ArtworkImage): ArtworkImage {
  return Object.assign({}, image, { preview: true });
}

/** The part of a movie or a show page that shows artwork: the hero images and the type list. */
interface PageArtwork {
  poster: ArtworkImage | null;
  backdrop: ArtworkImage | null;
  artwork?: DetailArtworkDto[];
}

/** The page caches that show artwork, by the query key of each kind of page. */
const PAGE_KEYS: readonly ((itemId: string) => readonly string[])[] = [
  (itemId) => ["movie-detail", itemId],
  (itemId) => ["tv-show", itemId],
];

/** The part of a cached show page that holds its seasons. */
interface ShowPage {
  seasons: TvSeasonDto[];
}

/** A season inside a cached show page, which is where the season page reads its artwork. */
function patchedSeason(
  season: TvSeasonDto,
  slot: string,
  candidate: ArtworkCandidate,
): TvSeasonDto {
  const placed = { image: candidate.image, width: candidate.width, height: candidate.height };
  const image = previewImage(candidate.image);
  return {
    ...season,
    ...(slot === "poster" ? { poster: image, poster_from_show: false } : {}),
    ...(slot === "fanart" ? { fanart: image } : {}),
    artwork: season.artwork.map((entry) => (entry.slot === slot ? { ...entry, ...placed } : entry)),
  };
}

/**
 * Patches the season `itemId` wherever a cached show page lists it. Returns the function that
 * puts each page back (only while nothing newer replaced it, when `onlyIfUnchanged`).
 */
function patchSeasons(
  queryClient: QueryClient,
  itemId: string,
  slot: string,
  candidate: ArtworkCandidate,
  onlyIfUnchanged: boolean,
): (() => void)[] {
  const undo: (() => void)[] = [];
  const pages = queryClient.getQueriesData<ShowPage>({ queryKey: ["tv-show"] });
  for (const [key, before] of pages as [QueryKey, ShowPage | undefined][]) {
    if (before === undefined || !Array.isArray(before.seasons)) continue;
    if (!before.seasons.some((season) => season.id === itemId)) continue;
    queryClient.setQueryData<ShowPage>(key, {
      ...before,
      seasons: before.seasons.map((season) =>
        season.id === itemId ? patchedSeason(season, slot, candidate) : season,
      ),
    });
    const shown = queryClient.getQueryData<ShowPage>(key);
    undo.push(() => {
      if (!onlyIfUnchanged || queryClient.getQueryData<ShowPage>(key) === shown) {
        queryClient.setQueryData<ShowPage>(key, before);
      }
    });
  }
  return undo;
}

function patched(before: PageArtwork, slot: string, candidate: ArtworkCandidate): PageArtwork {
  const placed = {
    image: candidate.image,
    width: candidate.width,
    height: candidate.height,
  };
  const next: PageArtwork = { ...before };
  // The hero reads its images from these two fields, not from the list of types.
  if (slot === "poster") next.poster = previewImage(candidate.image);
  if (slot === "fanart") next.backdrop = previewImage(candidate.image);
  if (before.artwork !== undefined) {
    const known = before.artwork.some((entry) => entry.slot === slot);
    next.artwork = known
      ? before.artwork.map((entry) => (entry.slot === slot ? { ...entry, ...placed } : entry))
      : [...before.artwork, { slot, file_name: null, size_bytes: null, online: true, ...placed }];
  }
  return next;
}

/**
 * Shows a chosen image in its slot on the page (the hero and the artwork list of a movie, a show
 * or a season) before the backend has written it. Returns the function that puts the page's old image
 * back, for a refusal. A page that is not cached is left alone.
 */
export function patchMovieArtwork(
  queryClient: QueryClient,
  itemId: string,
  slot: string,
  candidate: ArtworkCandidate,
): () => void {
  const undo: (() => void)[] = [];
  for (const keyOf of PAGE_KEYS) {
    const key = keyOf(itemId);
    const before = queryClient.getQueryData<PageArtwork>(key);
    if (before === undefined) continue;
    queryClient.setQueryData<PageArtwork>(key, patched(before, slot, candidate));
    undo.push(() => queryClient.setQueryData<PageArtwork>(key, before));
  }
  undo.push(...patchSeasons(queryClient, itemId, slot, candidate, false));
  return () => {
    for (const restore of undo) restore();
  };
}
