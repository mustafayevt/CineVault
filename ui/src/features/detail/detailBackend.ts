import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  DetailArtworkDto,
  DetailCreditDto,
  DetailFileDto,
  DetailRatingDto,
  DetailStreamDto,
  DetailSubtitleDto,
  DetailTrailerDto,
  DetailVideoDto,
  CastPortraitDto,
  MovieDetailDto,
  NfoPreviewDto,
  PosterDto,
  TechBadgeDto,
} from "../../bindings";
import { restoreWatchedStates } from "../curation/watchedRestore";
import type { WatchedChange } from "../curation/watchedRestore";
import { createSyntheticDetailBackend } from "./syntheticDetail";

export type MovieDetail = MovieDetailDto;
export type TechBadge = TechBadgeDto;
export type DetailCredit = DetailCreditDto;
export type DetailVideo = DetailVideoDto;
export type DetailStream = DetailStreamDto;
export type DetailFile = DetailFileDto;
export type DetailRating = DetailRatingDto;
export type DetailArtwork = DetailArtworkDto;
export type DetailTrailer = DetailTrailerDto;
export type DetailSubtitle = DetailSubtitleDto;
export type NfoPreview = NfoPreviewDto;
/** An image by content key, with what is known to paint it before it loads. */
/**
 * An image of a title page. `preview` marks a chosen candidate shown before its file is written
 * (see `artworkOptimistic`), which loads from a URL of its own.
 */
export type DetailImageRef = PosterDto & { readonly preview?: boolean };
/** A portrait fetched for a person of the credits. */
export type CastPortrait = CastPortraitDto;

/** A refusal from the backend: `message` is ready to show, `code` says why (`not_found`, ...). */
export class DetailRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "DetailRequestError";
    this.code = code;
  }
}

/** Where the detail page gets its movie: the database in the desktop app, fixtures in a browser. */
export interface MovieDetailBackend {
  movie: (id: string) => Promise<MovieDetail>;
  /** The NFO a rewrite would write, and whether the file on disk already says the same. */
  nfo: (id: string) => Promise<NfoPreview>;
  /**
   * Marks the movie watched or not; resolves to the state it had before (empty when nothing
   * changed), which `restoreWatched` puts back for an Undo.
   */
  setWatched: (id: string, watched: boolean) => Promise<WatchedChange>;
  /** Puts back the states a `setWatched` reported: flag, play count and last-watched date. */
  restoreWatched: (previous: WatchedChange) => Promise<void>;
  /**
   * The portraits of a title's cast that no file in `.actors` covers, fetched into the app's
   * thumbnail cache (the ones that are ready; asking again finds the rest).
   */
  castPortraits: (id: string) => Promise<CastPortrait[]>;
  /** The IMDb ID of a person by their TMDB ID, read once and stored; `null` when unknown. */
  personImdbId: (tmdbId: string) => Promise<string | null>;
}

type Result<T> =
  { status: "ok"; data: T } | { status: "error"; error: { code: string; message: string } };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new DetailRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriDetailBackend: MovieDetailBackend = {
  movie: async (id) => unwrap(await commands.getMovieDetail(id)),
  nfo: async (id) => unwrap(await commands.previewMovieNfo(id)),
  setWatched: async (id, watched) => unwrap(await commands.setMovieWatched(id, watched)).previous,
  restoreWatched: restoreWatchedStates,
  castPortraits: async (id) => unwrap(await commands.getCastPortraits(id)),
  personImdbId: async (tmdbId) => unwrap(await commands.getPersonImdbId(tmdbId)),
};

let override: MovieDetailBackend | null = null;
let fallback: MovieDetailBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setDetailBackend(backend: MovieDetailBackend | null): void {
  override = backend;
}

export function currentDetailBackend(): MovieDetailBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriDetailBackend;
  // A plain browser has no database: it shows one fixture movie so the page can be developed.
  fallback ??= createSyntheticDetailBackend();
  return fallback;
}
