import type { QualityTier } from "../../components/QualityBadge";
import type { FeedKind, TorrentRelease } from "./torrentBackend";

/** The view settings of the feed, kept by the page while a search is shown. */
export interface FeedView {
  kind: FeedKind;
  /** Hide camera, telesync, telecine and screener copies (on by default). */
  hideLow: boolean;
  tier: "all" | QualityTier;
}

export const DEFAULT_FEED_VIEW: FeedView = { kind: "all", hideLow: true, tier: "all" };

const LOW_QUALITY = new Set(["cam", "telesync", "telecine", "screener"]);

/** Whether a release is a camera, telesync, telecine or screener copy. */
export function isLowQuality(release: Pick<TorrentRelease, "flags">): boolean {
  return release.flags.some((flag) => LOW_QUALITY.has(flag.kind));
}

/** A page about the title on another site. */
export interface TitleLink {
  label: string;
  url: string;
}

/**
 * The pages a matched title opens: TMDB, IMDb (from TMDB's external IDs), Letterboxd for a movie
 * (its `/tmdb/<id>` address sends to the film's page) and Trakt (its search by TMDB ID sends to
 * the title). A show has no Letterboxd page.
 */
export function titleLinks(
  kind: "movie" | "show",
  tmdbId: number,
  imdbId: string | null,
): TitleLink[] {
  const links: TitleLink[] = [
    {
      label: "TMDB",
      url: `https://www.themoviedb.org/${kind === "movie" ? "movie" : "tv"}/${tmdbId}`,
    },
  ];
  if (imdbId !== null && /^tt\d+$/.test(imdbId)) {
    links.push({ label: "IMDb", url: `https://www.imdb.com/title/${imdbId}/` });
  }
  if (kind === "movie") {
    links.push({ label: "Letterboxd", url: `https://letterboxd.com/tmdb/${tmdbId}/` });
  }
  links.push({
    label: "Trakt",
    url: `https://trakt.tv/search/tmdb/${tmdbId}?id_type=${kind === "movie" ? "movie" : "show"}`,
  });
  return links;
}

/** The details panel's width (FeedPanel's w-[380px]). */
const PANEL_WIDTH = 380;
/** Room the table needs besides its columns: the selection edge, padding and a scrollbar. */
const TABLE_CHROME = 32;
/** What always stays beside the panel: the name at its narrowest and the row's buttons. */
const ALWAYS_SHOWN = 160 + 148;
/**
 * Columns an open panel hides, in this order, until the rest fits beside it (the panel repeats
 * them), so no column ever sits under the panel. Widths match ReleaseTable's.
 */
const HIDE_ORDER: readonly [string, number][] = [
  ["source", 120],
  ["age", 74],
  ["size", 90],
  ["seeders", 100],
  ["quality", 104],
];

/** The columns to hide so the table fits in `width` minus the panel. */
export function columnsBesidePanel(width: number): ReadonlySet<string> {
  const room = width - PANEL_WIDTH - TABLE_CHROME;
  let needed = ALWAYS_SHOWN + HIDE_ORDER.reduce((sum, [, size]) => sum + size, 0);
  const hidden = new Set<string>();
  for (const [id, size] of HIDE_ORDER) {
    if (needed <= room) break;
    hidden.add(id);
    needed -= size;
  }
  return hidden;
}
