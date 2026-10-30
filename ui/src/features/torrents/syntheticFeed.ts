import type {
  ProviderEvent,
  TorrentFeedItemDto,
  TorrentFeedRowDto,
  TorrentReleaseDto,
} from "../../bindings";
import { CATALOG } from "./syntheticCatalog";
import { TorrentRequestError } from "./torrentBackend";
import type {
  FeedKind,
  FeedLookup,
  FeedLookupRequest,
  FeedRequest,
  FeedSource,
  FeedStart,
} from "./torrentBackend";

/**
 * The synthetic "Latest releases" feed (`bun run dev`, `bun run shots` and the tests): sources
 * with pages (YTS three, EZTV and Knaben two), sources with one page (The Pirate Bay, Nyaa),
 * Torrentio that has no feed and an indexer that refuses a search without words. Titles of the
 * catalog are found by the lookup (Blade Runner 2049 in the library at 1080p); an invented one
 * is a weak match and one is not found at all. No network.
 */

const MINUTE = 60_000;
const GB = 1_000_000_000;

interface FeedSourceSpec {
  id: string;
  name: string;
  kind: "built-in" | "indexer";
  movies: boolean;
  tv: boolean;
  /** Pages the source has; 0 means it has no feed. */
  pages: number;
  /** Refuses a search without words. */
  needsWords?: boolean;
  delay: number;
}

export const FEED_SOURCES: readonly FeedSourceSpec[] = [
  {
    id: "torrentio",
    name: "Torrentio",
    kind: "built-in",
    movies: true,
    tv: true,
    pages: 0,
    delay: 0,
  },
  { id: "yts", name: "YTS", kind: "built-in", movies: true, tv: false, pages: 3, delay: 250 },
  { id: "eztv", name: "EZTV", kind: "built-in", movies: false, tv: true, pages: 2, delay: 300 },
  {
    id: "apibay",
    name: "The Pirate Bay",
    kind: "built-in",
    movies: true,
    tv: true,
    pages: 1,
    delay: 700,
  },
  { id: "knaben", name: "Knaben", kind: "built-in", movies: true, tv: true, pages: 2, delay: 500 },
  { id: "nyaa", name: "Nyaa", kind: "built-in", movies: false, tv: true, pages: 1, delay: 400 },
  {
    id: "idx-1337x",
    name: "1337x (indexer)",
    kind: "indexer",
    movies: true,
    tv: true,
    pages: 1,
    needsWords: true,
    delay: 300,
  },
];

interface Spec {
  name: string;
  kind: "movie" | "show";
  title: string;
  year: number | null;
  season?: number;
  episode?: number;
  resolution: "2160p" | "1080p" | "720p" | "480p" | "unknown";
  source: string | null;
  hdr?: string | null;
  size: number;
  seeders: number;
  imdb?: string;
  cam?: boolean;
  /** Matches the library title of this name. */
  owned?: { itemId: string; name: string; upgrade: "better" | "same" | "worse" | null };
}

/** The releases each source lists, page by page (index 0 is the newest). */
const PAGES: Readonly<Record<string, readonly (readonly Spec[])[]>> = {
  yts: [
    [
      {
        name: "Blade Runner 2049 (2017) 2160p bluray x265 10bit YTS",
        kind: "movie",
        title: "Blade Runner 2049",
        year: 2017,
        resolution: "2160p",
        source: "BluRay",
        hdr: "HDR10",
        size: 9.4 * GB,
        seeders: 412,
        imdb: "tt1856101",
        owned: { itemId: "1", name: "Blade Runner 2049 (2017)", upgrade: "better" },
      },
      {
        name: "Past Lives (2023) 1080p web x264 YTS",
        kind: "movie",
        title: "Past Lives",
        year: 2023,
        resolution: "1080p",
        source: "WEB-DL",
        size: 1.9 * GB,
        seeders: 96,
        imdb: "tt13238346",
      },
      {
        name: "Harbour Lights (2026) 720p web x264 YTS",
        kind: "movie",
        title: "Harbour Lights",
        year: 2026,
        resolution: "720p",
        source: "WEB-DL",
        size: 0.9 * GB,
        seeders: 12,
      },
    ],
    [
      {
        name: "Poor Things (2023) 1080p bluray x264 YTS",
        kind: "movie",
        title: "Poor Things",
        year: 2023,
        resolution: "1080p",
        source: "BluRay",
        size: 2.4 * GB,
        seeders: 150,
        imdb: "tt14230458",
      },
      {
        name: "Blade II (2002) 1080p web x264 YTS",
        kind: "movie",
        title: "Blade II",
        year: 2002,
        resolution: "1080p",
        source: "WEB-DL",
        size: 1.7 * GB,
        seeders: 8,
      },
    ],
    [
      {
        name: "Heat (1995) 2160p bluray x265 10bit YTS",
        kind: "movie",
        title: "Heat",
        year: 1995,
        resolution: "2160p",
        source: "BluRay",
        hdr: "HDR10",
        size: 8.8 * GB,
        seeders: 230,
        imdb: "tt0113277",
      },
    ],
  ],
  eztv: [
    [
      {
        name: "Severance.S02E10.1080p.ATVP.WEB-DL.DDP5.1.H.264-GRP",
        kind: "show",
        title: "Severance",
        year: null,
        season: 2,
        episode: 10,
        resolution: "1080p",
        source: "WEB-DL",
        size: 2.9 * GB,
        seeders: 880,
        owned: { itemId: "200", name: "Severance (2022)", upgrade: null },
      },
      {
        name: "Night.Bakers.S02E01.720p.WEB.h264-GRP",
        kind: "show",
        title: "Night Bakers",
        year: null,
        season: 2,
        episode: 1,
        resolution: "720p",
        source: "WEB-DL",
        size: 0.7 * GB,
        seeders: 41,
      },
    ],
    [
      {
        name: "Night.Bakers.S01E08.1080p.WEB.h264-GRP",
        kind: "show",
        title: "Night Bakers",
        year: null,
        season: 1,
        episode: 8,
        resolution: "1080p",
        source: "WEB-DL",
        size: 1.4 * GB,
        seeders: 22,
      },
    ],
  ],
  apibay: [
    [
      {
        name: "Dune.Part.Two.2024.HDCAM.x264-GRP",
        kind: "movie",
        title: "Dune Part Two",
        year: 2024,
        resolution: "720p",
        source: "CAM",
        size: 1.2 * GB,
        seeders: 64,
        cam: true,
      },
      {
        name: "Blade.Runner.2049.2017.1080p.BluRay.x264-GRP",
        kind: "movie",
        title: "Blade Runner 2049",
        year: 2017,
        resolution: "1080p",
        source: "BluRay",
        size: 12.1 * GB,
        seeders: 77,
        owned: { itemId: "1", name: "Blade Runner 2049 (2017)", upgrade: "same" },
      },
    ],
  ],
  knaben: [
    [
      {
        name: "Oppenheimer.2023.2160p.UHD.BluRay.REMUX.DV.HDR.HEVC.Atmos-GRP",
        kind: "movie",
        title: "Oppenheimer",
        year: 2023,
        resolution: "2160p",
        source: "Remux",
        hdr: "DV",
        size: 78 * GB,
        seeders: 54,
      },
      {
        name: "Severance.S02E09.2160p.ATVP.WEB-DL.DV.HDR.H.265-GRP",
        kind: "show",
        title: "Severance",
        year: null,
        season: 2,
        episode: 9,
        resolution: "2160p",
        source: "WEB-DL",
        hdr: "DV",
        size: 6.4 * GB,
        seeders: 310,
        owned: { itemId: "200", name: "Severance (2022)", upgrade: null },
      },
    ],
    [
      {
        name: "Alien.1979.Directors.Cut.1080p.BluRay.x264-GRP",
        kind: "movie",
        title: "Alien",
        year: 1979,
        resolution: "1080p",
        source: "BluRay",
        size: 11 * GB,
        seeders: 39,
      },
    ],
  ],
  nyaa: [
    [
      {
        name: "[Group] Starfield Couriers - 07 (1080p) [ABCD1234].mkv",
        kind: "show",
        title: "Starfield Couriers",
        year: null,
        episode: 7,
        resolution: "1080p",
        source: null,
        size: 1.4 * GB,
        seeders: 120,
      },
    ],
  ],
};

function hashOf(name: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < name.length; i += 1) {
    hash = Math.imul(hash ^ name.charCodeAt(i), 16777619) >>> 0;
  }
  return hash.toString(16).padStart(8, "0").repeat(5);
}

function matchKey(title: string): string {
  return title.toLowerCase().replace(/[^a-z0-9]+/g, "");
}

function rowOf(spec: Spec, source: string, index: number): TorrentFeedRowDto {
  const id = hashOf(spec.name);
  const release: TorrentReleaseDto = {
    id,
    absorbs: [],
    name: spec.name,
    size: spec.size,
    seeders: spec.seeders,
    leechers: Math.round(spec.seeders / 4),
    published: Date.now() - (index + 1) * 37 * MINUTE,
    sources: [source],
    pages: [],
    magnet: `magnet:?xt=urn:btih:${id}`,
    torrentUrl: null,
    infoHash: id,
    resolution: spec.resolution,
    source: spec.source,
    codec: null,
    hdr: spec.hdr ?? null,
    audio: null,
    pack: null,
    flags: spec.cam === true ? [{ kind: "cam", text: "Camera recording" }] : [],
    upgrade: spec.owned?.upgrade ?? null,
    matching: true,
  };
  return {
    release,
    kind: spec.kind,
    title: spec.title,
    year: spec.year,
    season: spec.season ?? null,
    episode: spec.episode ?? null,
    imdbId: spec.imdb ?? null,
    titleKey:
      spec.kind === "movie"
        ? `movie|${matchKey(spec.title)}|${spec.year ?? ""}`
        : `show|${matchKey(spec.title)}`,
    inLibrary:
      spec.owned === undefined ? null : { itemId: spec.owned.itemId, name: spec.owned.name },
  };
}

function admits(kind: FeedKind, row: TorrentFeedRowDto): boolean {
  return kind === "all" || (kind === "movies" ? row.kind === "movie" : row.kind === "show");
}

function takesPart(spec: FeedSourceSpec, kind: FeedKind): "yes" | "movies-only" | "tv-only" {
  if (kind === "movies" && !spec.movies) return "tv-only";
  if (kind === "tv" && !spec.tv) return "movies-only";
  return "yes";
}

export interface SyntheticFeedOptions {
  /** How fast the sources answer: 1 is real time, 0.01 for tests. */
  speed?: number;
  /** The lookup fails as without a TMDB key. */
  noKey?: boolean;
}

export interface SyntheticFeed {
  latest: (
    request: FeedRequest,
    onEvent: (event: ProviderEvent<TorrentFeedItemDto>) => void,
  ) => Promise<FeedStart>;
  cancelLatest: (searchId: string) => void;
  feedLookup: (request: FeedLookupRequest) => Promise<FeedLookup>;
  /** Every feed request and every lookup, for the tests. */
  feedCalls: { requests: FeedRequest[]; lookups: FeedLookupRequest[] };
}

/** The synthetic feed: a cursor per source and kind, rows by page, cancellable timers. */
export function createSyntheticFeed(options: SyntheticFeedOptions = {}): SyntheticFeed {
  const speed = options.speed ?? 1;
  const feedCalls: SyntheticFeed["feedCalls"] = { requests: [], lookups: [] };
  // The next page of each source, per kind; reset by a refresh.
  let cursors: Record<string, number> = {};
  let generation = 1;
  const timers = new Map<string, number[]>();

  const latest: SyntheticFeed["latest"] = (request, onEvent) => {
    feedCalls.requests.push(request);
    if (request.refresh) {
      cursors = {};
      generation += 1;
    }
    const sources: FeedSource[] = [];
    const asked: FeedSourceSpec[] = [];
    for (const spec of FEED_SOURCES) {
      const key = `${request.kind}:${spec.id}`;
      const part = takesPart(spec, request.kind);
      let state: string;
      if (spec.pages === 0) state = "unsupported";
      else if (part !== "yes") state = part;
      else if ((cursors[key] ?? 0) >= spec.pages) state = "done";
      else if (cursors[key] === -1) state = "unsupported";
      else if (!request.more && (cursors[key] ?? 0) > 0) state = "more";
      else state = "asked";
      sources.push({ id: spec.id, name: spec.name, kind: spec.kind, state });
      if (state === "asked") asked.push(spec);
    }
    const handles: number[] = [];
    timers.set(request.searchId, handles);
    let pending = asked.length;
    const finish = () => {
      pending -= 1;
      if (pending === 0) onEvent({ kind: "done" });
    };
    for (const spec of asked) {
      onEvent({ kind: "started", provider: spec.id });
      handles.push(
        window.setTimeout(() => {
          const key = `${request.kind}:${spec.id}`;
          if (spec.needsWords === true) {
            cursors[key] = -1;
            onEvent({
              kind: "results",
              provider: spec.id,
              items: [{ type: "sourceState", state: "unsupported" }],
            });
            finish();
            return;
          }
          const page = cursors[key] ?? 0;
          const rows = (PAGES[spec.id]?.[page] ?? [])
            .map((row, index) => rowOf(row, spec.id, page * 10 + index))
            .filter((row) => admits(request.kind, row));
          cursors[key] = page + 1;
          const more = page + 1 < spec.pages;
          const items: TorrentFeedItemDto[] = [
            ...rows.map((row): TorrentFeedItemDto => ({ type: "row", row })),
            { type: "sourceState", state: more ? "more" : "done" },
          ];
          onEvent({ kind: "results", provider: spec.id, items });
          finish();
        }, spec.delay * speed),
      );
    }
    if (asked.length === 0) onEvent({ kind: "done" });
    // The rows already read come with the start reply of a first page.
    const kept: TorrentFeedRowDto[] = [];
    if (!request.more && !request.refresh) {
      for (const spec of FEED_SOURCES) {
        const read = cursors[`${request.kind}:${spec.id}`] ?? 0;
        if (read <= 0 || asked.includes(spec)) continue;
        for (let page = 0; page < read; page += 1) {
          for (const [index, row] of (PAGES[spec.id]?.[page] ?? []).entries()) {
            const dto = rowOf(row, spec.id, page * 10 + index);
            if (admits(request.kind, dto)) kept.push(dto);
          }
        }
      }
    }
    return Promise.resolve({ generation: String(generation), sources, rows: kept });
  };

  return {
    feedCalls,
    latest,
    cancelLatest: (searchId) => {
      for (const handle of timers.get(searchId) ?? []) window.clearTimeout(handle);
      timers.delete(searchId);
    },
    feedLookup: (request) => {
      feedCalls.lookups.push(request);
      if (options.noKey === true) {
        return Promise.reject(
          new TorrentRequestError(
            "needs_key",
            "TMDB needs an API key. Add yours in Settings, Scraping and providers.",
          ),
        );
      }
      const kind = request.kind === "show" ? "show" : "movie";
      const wanted = matchKey(request.title);
      const exact = CATALOG.find(
        (entry) =>
          entry.kind === kind &&
          (entry.imdb === request.imdbId ||
            (matchKey(entry.title) === wanted &&
              (request.year === null || entry.year === request.year))),
      );
      const near =
        exact ??
        CATALOG.find(
          (entry) =>
            entry.kind === kind &&
            wanted.length > 3 &&
            matchKey(entry.title).startsWith(wanted.slice(0, 4)),
        );
      if (near === undefined) return Promise.resolve({ hit: null, weak: true });
      const confidence = exact === undefined ? 0.42 : 0.96;
      return Promise.resolve({
        hit: {
          id: `${near.kind}:${near.tmdbId}`,
          kind: near.kind,
          tmdbId: near.tmdbId,
          title: near.title,
          originalTitle: near.original,
          year: near.year,
          overview: near.overview,
          posterKey: null,
          imdbId: near.imdb,
          confidence,
          inLibrary:
            near.inLibrary === null ? null : { itemId: String(near.tmdbId), name: near.inLibrary },
        },
        weak: confidence < 0.75,
      });
    },
  };
}
