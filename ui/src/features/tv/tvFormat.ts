import { t } from "../../i18n/catalog";
import { runtimeLabel } from "../detail/detailFormat";
import type { ShowDetail, TvEpisodeRow, TvMissingEpisode } from "./tvBackend";

/**
 * The name an episode goes by: its title, or `Episode 3` when it has none (a file name without a
 * title in it, before the episode is scraped).
 */
export function episodeName(episode: { title: string; number: number }): string {
  const title = episode.title.trim();
  return title === "" ? t("tv.episode.untitled", { number: String(episode.number) }) : title;
}

/** `Season 2`, or `Specials` for season 0. */
export function seasonTitle(number: number): string {
  return number === 0 ? t("tv.season.specials") : t("tv.season.name", { number: String(number) });
}

/** One line of a season list: an episode of the library, or one the library lacks. */
export type SeasonRow =
  | { kind: "episode"; number: number; episode: TvEpisodeRow }
  | { kind: "missing"; number: number; missing: TvMissingEpisode }
  | { kind: "unaired"; number: number; missing: TvMissingEpisode };

/**
 * How many episodes the library has: one per season and number, so a second file of an episode
 * (a REPACK beside the original) is a duplicate, not another episode.
 */
export function episodeCount(rows: readonly Pick<TvEpisodeRow, "season" | "number">[]): number {
  return new Set(rows.map((row) => `${row.season}:${row.number}`)).size;
}

/** `2022–`, `2008–2013`, `2019`; empty when the year is unknown. */
function yearSpan(year: number, yearEnd: number, status: string): string {
  if (year <= 0) return "";
  if (yearEnd > year) return `${year}–${yearEnd}`;
  return status === "continuing" ? `${year}–` : String(year);
}

/** The line under a show's title: `2022– · TV-MA · 50 min · Science Fiction · 2 seasons · 9 episodes`. */
export function showMetaLine(detail: ShowDetail): string {
  const seasons = detail.seasons.filter(
    (season) => season.number > 0 && season.episodes > 0,
  ).length;
  return [
    yearSpan(detail.year, detail.year_end, detail.status),
    detail.certification,
    runtimeLabel(detail.runtime_minutes),
    detail.genres.join(", "),
    seasons > 0 ? t("tv.meta.seasons", { count: seasons }) : "",
    detail.episodes.length > 0
      ? t("tv.meta.episodes", { count: episodeCount(detail.episodes) })
      : "",
  ]
    .filter((part) => part !== "")
    .join(" · ");
}
