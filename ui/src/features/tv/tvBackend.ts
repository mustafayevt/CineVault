import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  AssignEpisodeDto,
  EpisodeDetailDto,
  EpisodeScrapeReportDto,
  EpisodeWriteDto,
  EpisodeHitDto,
  EpisodeOrderingDto,
  EpisodePickDto,
  OrderingReportDto,
  ShowDetailDto,
  ShowOrderingsDto,
  TvEpisodeLinkDto,
  TvEpisodeNumberDto,
  TvEpisodeRowDto,
  TvGuestDto,
  TvMissingEpisodeDto,
  TvSeasonDto,
  TvUncategorisedDto,
} from "../../bindings";
import { restoreWatchedStates } from "../curation/watchedRestore";
import type { WatchedChange } from "../curation/watchedRestore";
import { DetailRequestError } from "../detail/detailBackend";
import type { NfoPreview } from "../detail/detailBackend";
import { createSyntheticTvBackend } from "./syntheticTv";

export type ShowDetail = ShowDetailDto;
export type EpisodeDetail = EpisodeDetailDto;
export type TvSeason = TvSeasonDto;
export type TvEpisodeRow = TvEpisodeRowDto;
export type TvMissingEpisode = TvMissingEpisodeDto;
export type TvUncategorised = TvUncategorisedDto;
export type TvGuest = TvGuestDto;
export type TvEpisodeLink = TvEpisodeLinkDto;
export type TvEpisodeNumber = TvEpisodeNumberDto;
/** An episode ordering: its kind (`aired`, `dvd`, `absolute`, `alternate`) and the provider's name. */
export type ShowOrdering = EpisodeOrderingDto;
export type ShowOrderings = ShowOrderingsDto;
export type OrderingReport = OrderingReportDto;
/** An entry of the provider's episode list that fits a typed title. */
export type EpisodeHit = EpisodeHitDto;
export type EpisodePick = EpisodePickDto;
export type AssignedEpisode = AssignEpisodeDto;
/** What scraping the episodes of a show did: how many matched, changed or had no counterpart. */
export type EpisodeScrapeReport = EpisodeScrapeReportDto;
/** What a scrape wrote into one episode. */
export type EpisodeWrite = EpisodeWriteDto;

/** Where the TV pages get their shows: the database in the desktop app, fixtures in a browser. */
export interface TvBackend {
  show: (id: string) => Promise<ShowDetail>;
  episode: (id: string) => Promise<EpisodeDetail>;
  /** The `tvshow.nfo` a rewrite would write, and whether the file on disk says the same. */
  showNfo: (id: string) => Promise<NfoPreview>;
  /** The NFO of the video file the episode sits in. */
  episodeNfo: (id: string) => Promise<NfoPreview>;
  /**
   * Marks episodes watched or not; resolves to the state each changed episode had before, which
   * `restoreWatched` puts back for an Undo.
   */
  setWatched: (ids: readonly string[], watched: boolean) => Promise<WatchedChange>;
  /** Puts back the states a `setWatched` reported: flag, play count and last-watched date. */
  restoreWatched: (previous: WatchedChange) => Promise<void>;
  /** The ordering the show uses and the ones it can be switched to. */
  orderings: (id: string) => Promise<ShowOrderings>;
  /**
   * Switches the show to an ordering and rematches its episodes. Rejects with the code
   * `ordering_not_available` when the provider has no episodes for the show in that ordering.
   */
  setOrdering: (id: string, ordering: ShowOrdering) => Promise<OrderingReport>;
  /**
   * Searches the episode list of the show (in its ordering) by title. Rejects with the code
   * `not_available` when the provider has no list and `no_provider` when none is ready.
   */
  searchEpisodes: (showId: string, query: string) => Promise<readonly EpisodeHit[]>;
  /**
   * Gives an episode the numbers and IDs of a searched entry and scrapes the selected fields into
   * it. Rejects with `stale_pick` when the entry has left the provider's list.
   */
  assignEpisode: (episodeId: string, pick: EpisodePick) => Promise<AssignedEpisode>;
  /**
   * Scrapes the episodes of the show by number in its ordering, with the scrape settings'
   * fields. Rejects with `no_provider` or `not_available` when there is no list to read.
   */
  scrapeShowEpisodes: (showId: string) => Promise<EpisodeScrapeReport>;
  /**
   * Scrapes one episode. Rejects with `no_match` when the provider's list has nothing for its
   * number, which is the cue to search it by title.
   */
  scrapeEpisode: (episodeId: string) => Promise<EpisodeWrite>;
}

type Result<T> =
  { status: "ok"; data: T } | { status: "error"; error: { code: string; message: string } };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new DetailRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriTvBackend: TvBackend = {
  show: async (id) => unwrap(await commands.getShowDetail(id)),
  episode: async (id) => unwrap(await commands.getEpisodeDetail(id)),
  showNfo: async (id) => unwrap(await commands.previewShowNfo(id)),
  episodeNfo: async (id) => unwrap(await commands.previewEpisodeNfo(id)),
  setWatched: async (ids, watched) =>
    unwrap(await commands.setEpisodesWatched([...ids], watched)).previous,
  restoreWatched: restoreWatchedStates,
  orderings: async (id) => unwrap(await commands.getShowOrderings(id)),
  setOrdering: async (id, ordering) =>
    unwrap(
      await commands.setShowOrdering(id, { ordering: ordering.ordering, name: ordering.name }),
    ),
  searchEpisodes: async (showId, query) => unwrap(await commands.searchShowEpisodes(showId, query)),
  // `null` for the fields and the mode: the scrape settings decide.
  assignEpisode: async (episodeId, pick) =>
    unwrap(await commands.assignEpisode(episodeId, pick, null, null)),
  scrapeShowEpisodes: async (showId) =>
    unwrap(await commands.scrapeShowEpisodes(showId, null, null)),
  scrapeEpisode: async (episodeId) => unwrap(await commands.scrapeEpisode(episodeId, null, null)),
};

let override: TvBackend | null = null;
let fallback: TvBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setTvBackend(backend: TvBackend | null): void {
  override = backend;
}

export function currentTvBackend(): TvBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriTvBackend;
  // A plain browser has no database: it shows one fixture show so the pages can be developed.
  fallback ??= createSyntheticTvBackend();
  return fallback;
}
