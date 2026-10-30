import type { Channel } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import { isServer, transport } from "../../lib/transport";
import type {
  IndexerDto,
  IndexerManagerAccessDto,
  IndexerServiceStatusDto,
  OwnJackettDto,
  ProviderEvent,
  TorrentCommandError,
  TorrentEpisodeDto,
  TorrentLandingDto,
  TorrentLandingTitleDto,
  TorrentReleaseDto,
  TorrentSearchStartDto,
  TorrentSeasonDto,
  TorrentSourceStatusDto,
  TorrentTitleDetailsDto,
  TorrentTitleHitDto,
  TorrentTitleStartDto,
  TorznabTestDto,
} from "../../bindings";
import { channelTransport } from "../workspaces/channelTransport";
import type { ProviderSearchTransport } from "../workspaces/providerEvents";
import { createSyntheticTorrentBackend } from "./syntheticTorrents";
import type { TitleKind } from "./torrentsSearch";

export type TitleHit = TorrentTitleHitDto;
export type TitleDetails = TorrentTitleDetailsDto;
export type SeasonInfo = TorrentSeasonDto;
export type EpisodeInfo = TorrentEpisodeDto;
export type TorrentRelease = TorrentReleaseDto;
export type SourceStatus = TorrentSourceStatusDto;
export type TorznabTest = TorznabTestDto;
export type SearchStart = TorrentSearchStartDto;
export type IndexerServiceStatus = IndexerServiceStatusDto;
export type IndexerInfo = IndexerDto;
export type IndexerManagerAccess = IndexerManagerAccessDto;
/** A Jackett the person runs themselves. */
export type OwnJackett = OwnJackettDto;
export type Landing = TorrentLandingDto;
export type LandingTitle = TorrentLandingTitleDto;
/** One source of a release search, with the reason it is skipped, if it is. */
export type SourceInfo = TorrentSearchStartDto["sources"][number];

/** A refusal from the backend: `message` is ready to show, `code` says why. */
export class TorrentRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "TorrentRequestError";
    this.code = code;
  }
}

/** What a release search is for. */
export interface ReleaseSearchTarget {
  kind: TitleKind;
  tmdbId: number;
  /** For a show: none searches the whole show. */
  season?: number;
  /** For a show: the episode of `season`. */
  episode?: number;
}

/** Where the Torrent search page gets its titles, releases and settings. */
export interface TorrentBackend {
  /** False when nothing searches here (a plain browser), so the page says so. */
  readonly available: boolean;
  /** The streaming title search: movies and shows, best match first. */
  titleTransport: (
    onStart?: (start: TorrentTitleStartDto) => void,
  ) => ProviderSearchTransport<TitleHit>;
  details: (kind: TitleKind, tmdbId: number) => Promise<TitleDetails>;
  seasons: (tmdbId: number) => Promise<SeasonInfo[]>;
  episodes: (tmdbId: number, season: number) => Promise<EpisodeInfo[]>;
  /** The library strips of the landing: upgrades available and the newest titles. */
  landing: () => Promise<Landing>;
  /**
   * The streaming release search of a confirmed title and scope. `params` of the request are the
   * target; `onStart` receives every source with the reason it is skipped, if it is.
   */
  releaseTransport: (
    onStart?: (start: SearchStart) => void,
  ) => ProviderSearchTransport<TorrentRelease>;
  /** Hands a magnet link to the system's torrent app. */
  openMagnet: (magnet: string) => Promise<void>;
  /** Downloads a `.torrent` file and opens it. */
  openTorrentFile: (url: string, name: string) => Promise<void>;
  /** Opens a release's page on its source's site in the browser (`http` and `https` only). */
  openPage: (url: string) => Promise<void>;
  sourcesStatus: () => Promise<SourceStatus[]>;
  setSource: (id: string, on: boolean) => Promise<SourceStatus[]>;
  /** `apiKey` null tests with the saved key. */
  testTorznab: (url: string, apiKey: string | null) => Promise<TorznabTest>;
  /** `apiKey` null keeps the saved key, an empty text removes it. An empty address removes all. */
  setTorznab: (url: string, apiKey: string | null) => Promise<SourceStatus[]>;
  /**
   * Looks for a Jackett the person runs themselves (the saved instance's address, then
   * 127.0.0.1:9117), never CineVault's own service; `null` when none answers.
   */
  detectJackett: () => Promise<OwnJackett | null>;
  /** How the managed indexer service stands. */
  indexerStatus: () => Promise<IndexerServiceStatus>;
  /** Switches the indexer service on and makes sure it is downloaded and running. */
  installIndexer: () => Promise<IndexerServiceStatus>;
  /** Installs the newest release of the indexer service. */
  updateIndexer: () => Promise<IndexerServiceStatus>;
  /** Checks the indexers now. */
  checkSources: () => Promise<void>;
  /** Opens the service's own page and returns the admin password. */
  openIndexerManager: () => Promise<IndexerManagerAccess>;
  /** The indexers of the service with their health. */
  indexers: () => Promise<IndexerInfo[]>;
  /** Switches one indexer on or off for good. */
  setIndexer: (id: string, on: boolean) => Promise<IndexerInfo[]>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: TorrentCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new TorrentRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

/** The title search of the desktop app: `query` is the typed text. */
function titleStreamTransport(
  onStart?: (start: TorrentTitleStartDto) => void,
): ProviderSearchTransport<TitleHit> {
  return channelTransport<TitleHit>(async (request, channel) => {
    const start = unwrap(
      await commands.torrentTitleSearch(
        { searchId: request.searchId, query: request.query },
        channel as Channel<ProviderEvent<TorrentTitleHitDto>>,
      ),
    );
    onStart?.(start);
  });
}

/** The release search of the desktop app. */
function releaseStreamTransport(
  onStart?: (start: SearchStart) => void,
): ProviderSearchTransport<TorrentRelease> {
  return channelTransport<TorrentRelease>(async (request, channel) => {
    const params = request.params ?? {};
    // `attempt` only restarts the search ("Search again"); the backend does not read it.
    const season = typeof params.season === "number" ? params.season : null;
    const episode = typeof params.episode === "number" ? params.episode : null;
    const answer = await commands.torrentReleaseSearch(
      {
        searchId: request.searchId,
        kind: params.kind === "show" ? "show" : "movie",
        tmdbId: Number(params.tmdbId),
        season,
        episode,
      },
      channel as Channel<ProviderEvent<TorrentReleaseDto>>,
    );
    onStart?.(unwrap(answer));
  });
}

/** The backend of the desktop app: the sources and TMDB live in the Rust service. */
const tauriTorrentBackend: TorrentBackend = {
  available: true,
  titleTransport: titleStreamTransport,
  details: async (kind, tmdbId) => unwrap(await commands.torrentTitleDetails(kind, tmdbId)),
  seasons: async (tmdbId) => unwrap(await commands.torrentTitleSeasons(tmdbId)),
  episodes: async (tmdbId, season) => unwrap(await commands.torrentTitleEpisodes(tmdbId, season)),
  landing: async () => unwrap(await commands.torrentLanding()),
  releaseTransport: releaseStreamTransport,
  openMagnet: async (magnet) => void unwrap(await commands.torrentOpenMagnet(magnet)),
  openTorrentFile: async (url, name) =>
    void unwrap(await commands.torrentOpenTorrentFile(url, name)),
  openPage: async (url) => void unwrap(await commands.torrentOpenPage(url)),
  sourcesStatus: async () => unwrap(await commands.torrentSourcesStatus()),
  setSource: async (id, on) => unwrap(await commands.torrentSetSource(id, on)),
  testTorznab: async (url, apiKey) => unwrap(await commands.torrentTestTorznab(url, apiKey)),
  setTorznab: async (url, apiKey) => unwrap(await commands.torrentSetTorznab(url, apiKey)),
  detectJackett: async () => unwrap(await commands.torrentDetectJackett()),
  indexerStatus: async () => unwrap(await commands.torrentIndexerServiceStatus()),
  installIndexer: async () => unwrap(await commands.torrentInstallIndexerService()),
  updateIndexer: async () => unwrap(await commands.torrentUpdateIndexerService()),
  checkSources: async () => void unwrap(await commands.torrentCheckSources()),
  openIndexerManager: async () => unwrap(await commands.torrentOpenIndexerManager()),
  indexers: async () => unwrap(await commands.torrentIndexers()),
  setIndexer: async (id, on) => unwrap(await commands.torrentSetIndexer(id, on)),
};

/** The browser server: the client's own browser opens magnets, pages and .torrent files. */
const browserTorrentBackend: TorrentBackend = {
  ...tauriTorrentBackend,
  openMagnet: async (magnet) => {
    window.open(magnet, "_blank", "noopener");
  },
  openTorrentFile: async (url) => {
    window.open(url, "_blank", "noopener");
  },
  openPage: async (url) => {
    window.open(url, "_blank", "noopener");
  },
  openIndexerManager: async () => {
    throw new Error("The indexer manager runs on the server; open its address there.");
  },
};

let override: TorrentBackend | null = null;
let fallback: TorrentBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setTorrentBackend(backend: TorrentBackend | null): void {
  override = backend;
}

export function currentTorrentBackend(): TorrentBackend {
  if (override !== null) return override;
  if (transport() === "desktop") return tauriTorrentBackend;
  if (isServer()) return browserTorrentBackend;
  // A plain browser has no sources: it uses fixtures so the page can be developed.
  fallback ??= createSyntheticTorrentBackend();
  return fallback;
}
