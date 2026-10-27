import type {
  IndexerDto,
  IndexerServiceStatusDto,
  TorrentEpisodeDto,
  TorrentReleaseDto,
  TorrentSeasonDto,
  TorrentSourceStatusDto,
  TorrentTitleDetailsDto,
  TorrentTitleHitDto,
  TorznabTestDto,
} from "../../bindings";
import {
  createFakeProviderTransport,
  perSearchTransport,
} from "../workspaces/fakeProviderTransport";
import type { FakeProviderScript } from "../workspaces/fakeProviderTransport";
import type { ProviderSearchRequest } from "../workspaces/providerEvents";
import {
  CATALOG,
  SHOW_EPISODES,
  SOURCES,
  catalogTitle,
  syntheticLanding,
  syntheticReleases,
} from "./syntheticCatalog";
import type { CatalogTitle, SyntheticTarget } from "./syntheticCatalog";
import { TorrentRequestError } from "./torrentBackend";
import type { SearchStart, TorrentBackend } from "./torrentBackend";
import type { TitleKind } from "./torrentsSearch";

export { syntheticReleases } from "./syntheticCatalog";
export type { SyntheticTarget } from "./syntheticCatalog";

/**
 * The synthetic Torrent search backend (`bun run dev`, `bun run shots` and the tests): the titles
 * of `syntheticCatalog.ts`, their releases with each source's page, one source that fails until it
 * is retried (Knaben), sources that do not apply, the landing strips, and titles for the states
 * (every source failing, sources that never finish, nothing found). No network.
 */

const SECOND_MS = 1000;

function hitOf(entry: CatalogTitle, confidence: number): TorrentTitleHitDto {
  return {
    id: `${entry.kind}:${entry.tmdbId}`,
    kind: entry.kind,
    tmdbId: entry.tmdbId,
    title: entry.title,
    originalTitle: entry.original,
    year: entry.year,
    overview: entry.overview,
    posterKey: null,
    imdbId: entry.imdb,
    confidence,
    inLibrary:
      entry.inLibrary === null ? null : { itemId: String(entry.tmdbId), name: entry.inLibrary },
  };
}

/** How well a typed text fits a title: whole words first, then any part. */
function fit(query: string, title: string): number {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return 0;
  const lower = title.toLowerCase();
  const found = words.filter((word) => lower.includes(word)).length;
  if (found === 0) return 0;
  return Math.min(1, found / words.length) * (lower.startsWith(words[0] ?? "") ? 1 : 0.8);
}

/**
 * The confidence of a title for typed text, as the app ranks it: the fit (a trailing year read as
 * the year, a title of another year less fit), plus a small share for its votes.
 */
function syntheticConfidence(query: string, entry: CatalogTitle): number {
  const match = /^(.*\S)\s+(\d{4})$/.exec(query.trim());
  const year = match === null ? null : Number(match[2]);
  const plain = fit(query, entry.title);
  const dated =
    match === null || year === null
      ? 0
      : Math.max(0, fit(match[1] ?? "", entry.title) - (year === entry.year ? 0 : 0.3));
  const known = 0.1 * Math.min(1, Math.log(entry.votes + 1) / Math.log(10_001));
  const fitness = Math.max(plain, dated);
  return fitness === 0 ? 0 : Math.min(1, 0.9 * fitness + known);
}

// ---------------------------------------------------------------- the backend

/** The indexers of the synthetic service: two work, one needs a solver, one is unreachable. */
const SYNTHETIC_INDEXERS: readonly IndexerDto[] = [
  {
    id: "1337x",
    name: "1337x",
    state: "needsSolver",
    reason: "A browser challenge blocks it. A challenge solver address would let it through.",
    checkedAt: Date.UTC(2026, 9, 26, 8, 0, 0),
    userOff: false,
  },
  {
    id: "limetorrents",
    name: "LimeTorrents",
    state: "working",
    reason: null,
    checkedAt: Date.UTC(2026, 9, 26, 8, 0, 0),
    userOff: false,
  },
  {
    id: "torrentgalaxy",
    name: "TorrentGalaxy",
    state: "unreachable",
    reason: "The site did not answer.",
    checkedAt: Date.UTC(2026, 9, 26, 8, 0, 0),
    userOff: false,
  },
  {
    id: "yts",
    name: "YTS (indexer)",
    state: "working",
    reason: null,
    checkedAt: Date.UTC(2026, 9, 26, 8, 0, 0),
    userOff: false,
  },
];

export interface SyntheticTorrentOptions {
  /** Scales every delay; tests pass a small number. Default 1. */
  speed?: number;
  /** Source ids that fail (until retried) instead of answering. */
  failing?: readonly string[];
  /** Source ids that never answer in time (they time out after a while). */
  slow?: readonly string[];
  /** The state the indexer service starts in. Default `running`. */
  indexerState?: string;
  /** Whether a Jackett of the person's own answers on this computer. Default true. */
  ownJackett?: boolean;
}

export interface SyntheticTorrentCalls {
  magnets: string[];
  files: { url: string; name: string }[];
  /** The release pages opened in the browser. */
  pages: string[];
  switched: { id: string; on: boolean }[];
  torznab: { url: string; apiKey: string | null }[];
  /** What was asked of the indexer service: `install`, `update`, `check`, `open`, `set:<id>:<on>`. */
  indexer: string[];
}

export interface SyntheticTorrentBackend extends TorrentBackend {
  calls: SyntheticTorrentCalls;
}

/** How each source answers a release search of a target. */
function releaseScript(
  target: SyntheticTarget,
  options: Required<SyntheticTorrentOptions>,
): Record<string, FakeProviderScript<TorrentReleaseDto>> {
  const entry = catalogTitle(target.kind, target.tmdbId);
  const releases = syntheticReleases(target);
  const script: Record<string, FakeProviderScript<TorrentReleaseDto>> = {};
  const speed = options.speed;
  const failures: Readonly<Record<string, string>> = {
    torrentio: "unreachable",
    yts: "unreachable",
    eztv: "unreachable",
    apibay: "unreachable",
    knaben: "blocked",
    "idx-1337x": "blocked",
    "idx-limetorrents": "timeout",
  };
  for (const source of SOURCES) {
    if (!appliesTo(source, target)) continue;
    const items = releases.filter((release) => release.sources.includes(source.id));
    if (entry?.behaviour === "failing") {
      script[source.id] = {
        delayMs: source.delay * speed,
        fail: { reason: failures[source.id] ?? "unreachable", retryable: true },
        onRetry: {
          delayMs: source.delay * speed,
          fail: { reason: failures[source.id] ?? "unreachable", retryable: true },
        },
      };
    } else if (
      options.slow.includes(source.id) ||
      (entry?.behaviour === "slow" && !["torrentio", "yts"].includes(source.id))
    ) {
      script[source.id] = { delayMs: 600 * SECOND_MS * speed, items };
    } else if (options.failing.includes(source.id)) {
      script[source.id] = {
        delayMs: 300 * speed,
        fail: { reason: "rate-limited", retryable: true },
        onRetry: { delayMs: 200 * speed, items },
      };
    } else {
      script[source.id] = { delayMs: source.delay * speed, items };
    }
  }
  return script;
}

/** Whether a source takes part in a search for the target (Nyaa is for anime only). */
function appliesTo(source: (typeof SOURCES)[number], target: SyntheticTarget): boolean {
  if (source.id === "nyaa") return false;
  return target.kind === "movie" ? source.movies : source.tv;
}

/** Creates the synthetic backend. */
export function createSyntheticTorrentBackend(
  input: SyntheticTorrentOptions = {},
): SyntheticTorrentBackend {
  const options: Required<SyntheticTorrentOptions> = {
    speed: input.speed ?? 1,
    failing: input.failing ?? ["knaben"],
    slow: input.slow ?? [],
    indexerState: input.indexerState ?? "running",
    ownJackett: input.ownJackett ?? true,
  };
  const calls: SyntheticTorrentCalls = {
    magnets: [],
    files: [],
    pages: [],
    switched: [],
    torznab: [],
    indexer: [],
  };
  const running = options.indexerState === "running";
  let service: IndexerServiceStatusDto = {
    state: options.indexerState,
    version: options.indexerState === "notInstalled" ? null : "0.24.2903",
    latestVersion: running ? "0.24.3000" : null,
    port: running ? 9117 : null,
    working: 0,
    failed: 0,
    checking: false,
    reason: options.indexerState === "failed" ? "The port is taken" : null,
    defaultLanguages: ["en"],
    otherLanguages: running
      ? [
          { code: "ru", count: 7 },
          { code: "zh", count: 4 },
          { code: "es", count: 3 },
          { code: "fr", count: 2 },
        ]
      : [],
  };
  const JACKETT = "http://127.0.0.1:9117";
  const jackettFeed = `${JACKETT}/api/v2.0/indexers/all/results/torznab/api`;
  let savedFeed = "";
  let indexerRows: IndexerDto[] = running ? SYNTHETIC_INDEXERS.map((row) => ({ ...row })) : [];
  const tally = () => {
    service = {
      ...service,
      working: indexerRows.filter((row) => row.state === "working").length,
      failed: indexerRows.filter((row) =>
        ["needsSolver", "unreachable", "error"].includes(row.state),
      ).length,
    };
  };
  tally();
  let statuses: TorrentSourceStatusDto[] = SOURCES.map((source) => ({
    id: source.id,
    name: source.name,
    kind: source.kind,
    on: true,
    health: source.id === "knaben" ? "error" : "working",
    reason: source.id === "knaben" ? "Rate limited" : null,
    checkedAt: Date.UTC(2026, 9, 26, 8, 0, 0),
  }));

  const targetOf = (request: ProviderSearchRequest): SyntheticTarget => {
    const params = request.params ?? {};
    return {
      kind: params.kind === "show" ? "show" : "movie",
      tmdbId: Number(params.tmdbId),
      ...(typeof params.season === "number" ? { season: params.season } : {}),
      ...(typeof params.episode === "number" ? { episode: params.episode } : {}),
    };
  };

  return {
    available: true,
    calls,
    titleTransport: (onStart) =>
      perSearchTransport<TorrentTitleHitDto>((request) => {
        const text = request.query.trim();
        const rank = (kind: TitleKind) =>
          CATALOG.filter((entry) => entry.kind === kind)
            .map((entry) => ({ entry, score: syntheticConfidence(text, entry) }))
            .filter((row) => row.score > 0)
            .sort((a, b) => b.score - a.score)
            .map((row) => hitOf(row.entry, row.score));
        onStart?.({
          providers: [
            { key: "movies", name: "Movies" },
            { key: "shows", name: "Shows" },
          ],
        });
        return createFakeProviderTransport<TorrentTitleHitDto>({
          movies: { delayMs: 60 * options.speed, items: rank("movie") },
          shows: { delayMs: 120 * options.speed, items: rank("show") },
        });
      }),
    details: (kind, tmdbId) => {
      const entry = catalogTitle(kind, tmdbId);
      if (entry === undefined) {
        return Promise.reject(new TorrentRequestError("not_found", "That title was not found."));
      }
      const details: TorrentTitleDetailsDto = {
        kind,
        tmdbId,
        imdbId: entry.imdb,
        title: entry.title,
        originalTitle: entry.original,
        year: entry.year,
        runtimeMinutes: entry.runtime,
        anime: entry.anime,
        posterKey: null,
        inLibrary:
          entry.inLibrary === null ? null : { itemId: String(tmdbId), name: entry.inLibrary },
        backdropKey: null,
        certification: entry.certification,
        rating: entry.rating,
        genres: [...entry.genres],
        overview: entry.overview,
        libraryCopy: entry.copy === undefined ? null : { ...entry.copy },
        heldEpisodes: (entry.held ?? []).map((episode) => ({ ...episode })),
      };
      return Promise.resolve(details);
    },
    seasons: (tmdbId) => {
      const entry = catalogTitle("show", tmdbId);
      return Promise.resolve(
        Object.entries(entry === undefined ? {} : (SHOW_EPISODES[entry.tmdbId] ?? {})).map(
          ([number, episodes]): TorrentSeasonDto => ({
            number: Number(number),
            name: `Season ${number}`,
            episodeCount: episodes.length,
            airDate: episodes[0]?.aired ?? null,
          }),
        ),
      );
    },
    episodes: (tmdbId, season) => {
      const entry = catalogTitle("show", tmdbId);
      const list = entry === undefined ? [] : (SHOW_EPISODES[entry.tmdbId]?.[season] ?? []);
      return Promise.resolve(
        list.map((episode, index): TorrentEpisodeDto => ({
          number: index + 1,
          name: episode.name,
          airDate: episode.aired,
        })),
      );
    },
    landing: () => Promise.resolve(syntheticLanding()),
    releaseTransport: (onStart) =>
      perSearchTransport<TorrentReleaseDto>((request) => {
        const target = targetOf(request);
        const start: SearchStart = {
          sources: SOURCES.map((source) => {
            const skipped =
              source.id === "nyaa"
                ? "anime-only"
                : appliesTo(source, target)
                  ? null
                  : target.kind === "movie"
                    ? "tv-only"
                    : "movies-only";
            return { id: source.id, name: source.name, kind: source.kind, skipped };
          }),
        };
        onStart?.(start);
        return createFakeProviderTransport<TorrentReleaseDto>(releaseScript(target, options));
      }),
    openMagnet: (magnet) => {
      calls.magnets.push(magnet);
      return Promise.resolve();
    },
    openTorrentFile: (url, name) => {
      calls.files.push({ url, name });
      return Promise.resolve();
    },
    openPage: (url) => {
      calls.pages.push(url);
      return Promise.resolve();
    },
    sourcesStatus: () => Promise.resolve(statuses),
    setSource: (id, on) => {
      calls.switched.push({ id, on });
      statuses = statuses.map((status) =>
        status.id === id ? { ...status, on, health: on ? "untested" : "off" } : status,
      );
      return Promise.resolve(statuses);
    },
    testTorznab: (url, apiKey) => {
      calls.torznab.push({ url, apiKey });
      const result: TorznabTestDto = url.includes("rejected")
        ? { ok: false, message: "The key was rejected", caps: null }
        : url.includes("nothing")
          ? { ok: false, message: "Nothing answers at this address", caps: null }
          : url.includes("html")
            ? { ok: false, message: "This is not a Torznab feed", caps: null }
            : {
                ok: true,
                message: "Connected · 12 indexers",
                caps: {
                  search: true,
                  movieSearch: true,
                  tvSearch: true,
                  imdbIds: true,
                  indexers: 12,
                },
              };
      return Promise.resolve(result);
    },
    detectJackett: () =>
      Promise.resolve(
        options.ownJackett
          ? { baseUrl: JACKETT, feedUrl: jackettFeed, inUse: savedFeed === jackettFeed }
          : null,
      ),
    indexerStatus: () => Promise.resolve(service),
    installIndexer: () => {
      calls.indexer.push("install");
      service = {
        ...service,
        state: "running",
        version: "0.24.2903",
        port: 9117,
        reason: null,
      };
      indexerRows = SYNTHETIC_INDEXERS.map((row) => ({ ...row }));
      tally();
      return Promise.resolve(service);
    },
    updateIndexer: () => {
      calls.indexer.push("update");
      service = {
        ...service,
        version: service.latestVersion ?? service.version,
        latestVersion: null,
      };
      return Promise.resolve(service);
    },
    checkSources: () => {
      calls.indexer.push("check");
      return Promise.resolve();
    },
    openIndexerManager: () => {
      calls.indexer.push("open");
      return Promise.resolve({
        url: "http://127.0.0.1:9117/UI/Dashboard",
        password: "synthetic-admin-password",
      });
    },
    indexers: () => Promise.resolve(indexerRows),
    setIndexer: (id, on) => {
      calls.indexer.push(`set:${id}:${String(on)}`);
      indexerRows = indexerRows.map((row) =>
        row.id === id ? { ...row, userOff: !on, state: on ? "working" : "off", reason: null } : row,
      );
      tally();
      return Promise.resolve(indexerRows);
    },
    setTorznab: (url, apiKey) => {
      calls.torznab.push({ url, apiKey });
      savedFeed = url.trim();
      statuses = statuses.filter((status) => status.id !== "torznab");
      if (url.trim() !== "") {
        statuses = [
          ...statuses,
          {
            id: "torznab",
            name: "Your Torznab instance",
            kind: "torznab",
            on: true,
            health: "untested",
            reason: null,
            checkedAt: null,
          },
        ];
      }
      return Promise.resolve(statuses);
    },
  };
}
