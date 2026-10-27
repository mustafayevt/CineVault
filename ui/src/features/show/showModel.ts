import type { ShowDetail, TvEpisodeRow, TvMissingEpisode, TvSeason } from "../tv/tvBackend";
import { episodeCount } from "../tv/tvFormat";

/** An episode has a file on disk when its row names one. */
export function onDisk(episode: Pick<TvEpisodeRow, "file_name">): boolean {
  return episode.file_name !== "";
}

/**
 * The summed size of the distinct files of some episodes. A file that holds several episodes is
 * counted once, so a double episode does not count its size twice.
 */
export function distinctSize(episodes: readonly TvEpisodeRow[]): number | null {
  const seen = new Set<string>();
  let total = 0;
  let known = false;
  for (const episode of episodes) {
    if (episode.size_bytes === null || !onDisk(episode)) continue;
    const key = `${episode.file_name}:${episode.size_bytes}`;
    if (seen.has(key)) continue;
    seen.add(key);
    total += episode.size_bytes;
    known = true;
  }
  return known ? total : null;
}

/** `17 of 19 episodes`: what the library holds against what has aired (specials left out). */
export function showProgress(detail: ShowDetail): { owned: number; total: number } {
  const real = detail.episodes.filter((episode) => episode.season > 0);
  const owned = episodeCount(real);
  const missing = detail.missing.filter((episode) => episode.season > 0 && !episode.unaired).length;
  return { owned, total: owned + missing };
}

/** One row of a season: an episode of the library, or one it lacks. */
export type ShowRow =
  | { kind: "episode"; number: number; episode: TvEpisodeRow }
  | { kind: "missing"; number: number; missing: TvMissingEpisode }
  | { kind: "unaired"; number: number; missing: TvMissingEpisode };

/** The rows of a season in episode order: its episodes, then the ones it lacks (aired or not). */
export function rowsOf(detail: ShowDetail, season: number): ShowRow[] {
  const rows: ShowRow[] = detail.episodes
    .filter((episode) => episode.season === season)
    .map((episode) => ({ kind: "episode", number: episode.number, episode }));
  for (const missing of detail.missing) {
    if (missing.season !== season) continue;
    rows.push({
      kind: missing.unaired ? "unaired" : "missing",
      number: missing.number,
      missing,
    });
  }
  return rows.sort((a, b) => a.number - b.number);
}

/**
 * The seasons the page lists: the numbered ones that have episodes or a record of their own or
 * lack some, newest first (the one people open is the latest), then the specials when they have
 * anything to list.
 */
export function listedSeasons(detail: ShowDetail): {
  seasons: TvSeason[];
  specials: TvSeason | null;
} {
  const worth = (season: TvSeason) =>
    season.episodes > 0 || season.id !== null || season.missing > 0 || season.unaired > 0;
  const seasons = detail.seasons
    .filter((season) => season.number > 0 && worth(season))
    .sort((a, b) => b.number - a.number);
  const specials = detail.seasons.find((season) => season.number === 0 && worth(season)) ?? null;
  return { seasons, specials };
}

/** The season that opens with the page: the latest one that has files, else the latest. */
export function openingSeason(seasons: readonly TvSeason[]): number | null {
  return (seasons.find((season) => season.episodes > 0) ?? seasons[0])?.number ?? null;
}

/** What Play starts: the first unwatched episode that has a file, else the first that has one. */
export function playEpisode(detail: ShowDetail): TvEpisodeRow | null {
  const playable = detail.episodes
    .filter((episode) => episode.season > 0 && onDisk(episode))
    .sort((a, b) => a.season - b.season || a.number - b.number);
  return playable.find((episode) => !episode.watched) ?? playable[0] ?? null;
}
