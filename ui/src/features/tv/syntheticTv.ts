import { DetailRequestError } from "../detail/detailBackend";
import { syntheticAvailability } from "../detail/syntheticDetail";
import { syntheticScrapedLifecycle } from "../lifecycle/syntheticLifecycle";
import type { DetailVideo, NfoPreview } from "../detail/detailBackend";
import type {
  AssignedEpisode,
  EpisodeDetail,
  EpisodeHit,
  ShowDetail,
  ShowOrdering,
  TvBackend,
  TvEpisodeRow,
  TvMissingEpisode,
  TvSeason,
} from "./tvBackend";
import { episodeCount } from "./tvFormat";

/** The fixture show is scraped, with its rename waiting in Organize. */
const SHOW_LIFECYCLE = syntheticScrapedLifecycle(
  { index: 5, title: "Severance", year: 2022, unmatched: false, needsReview: false, show: true },
  false,
);

/**
 * A fixture show for a plain browser (no desktop backend) and for tests: a continuing show with
 * two seasons and specials, an episode with two files, an aired episode the library lacks, one
 * that has not aired yet, two files without numbering, guests and artwork.
 */

// ThumbHashes of gradients (produced by the Rust encoder) so images show a preview.
const BACKDROP_HASH = "d1690a2d0888887f7787778777788887887f87f887";
const POSTER_HASH = "8ce705050088887f78878787877887877680770778";
const LOGO_HASH = "4db6091d0688887f77877788777888877780770778";

const KEY = (n: number): string => n.toString(16).padStart(32, "0");
const image = (n: number, thumbhash: string) => ({ key: KEY(n), thumbhash, ambient: null });

const DAY_MS = 86_400_000;

/** The id of the fixture show; any other id is "not found". */
export const SYNTHETIC_SHOW_ID = "1";
/** The ids of the fixture episodes are `SYNTHETIC_EPISODE_BASE + n`. */
const SYNTHETIC_EPISODE_BASE = 100;

const FOLDER = "D:\\Shows\\Severance";
const SEASON_ONE_FOLDER = `${FOLDER}\\Season 1`;

interface Seed {
  season: number;
  number: number;
  title: string;
  aired: string;
  plot: string;
  height?: number;
  thumb?: boolean;
  isNew?: boolean;
  file?: string;
}

const SEEDS: readonly Seed[] = [
  {
    season: 0,
    number: 1,
    title: "Behind the Scenes",
    aired: "2022-03-04",
    plot: "The cast talks about the first season.",
    height: 720,
    file: "Severance - S00E01 - Behind the Scenes.mkv",
  },
  {
    season: 1,
    number: 1,
    title: "Good News About Hell",
    aired: "2022-02-18",
    plot: "Mark starts a day at Lumon Industries with a new colleague on his team.",
    thumb: true,
    file: "Severance - S01E01 - Good News About Hell.mkv",
  },
  {
    season: 1,
    number: 2,
    title: "Half Loop",
    aired: "2022-02-18",
    plot: "Helly tries to find a way out while Mark meets an old friend.",
    file: "Severance - S01E02 - Half Loop.mkv",
  },
  {
    season: 1,
    number: 3,
    title: "In Perpetuity",
    aired: "2022-02-25",
    plot: "The team visits the Perpetuity Wing.",
    height: 2160,
    file: "Severance - S01E03 - In Perpetuity.mkv",
  },
  {
    season: 1,
    number: 3,
    title: "In Perpetuity (repack)",
    aired: "2022-02-25",
    plot: "The team visits the Perpetuity Wing.",
    file: "Severance - S01E03 - In Perpetuity.REPACK.mkv",
  },
  {
    season: 1,
    number: 4,
    title: "The You You Are",
    aired: "2022-03-04",
    plot: "Mark finds out who he is outside the office.",
    isNew: true,
    file: "Severance - S01E04 - The You You Are.mkv",
  },
  {
    season: 2,
    number: 1,
    title: "Hello, Ms. Cobel",
    aired: "2025-01-17",
    plot: "Mark returns to the office with new rules.",
    thumb: true,
    isNew: true,
    file: "Severance - S02E01 - Hello, Ms. Cobel.mkv",
  },
  {
    season: 2,
    number: 2,
    title: "Goodbye, Mrs. Selvig",
    aired: "2025-01-24",
    plot: "Irving looks for answers.",
    file: "Severance - S02E02 - Goodbye, Mrs. Selvig.mkv",
  },
];

const MISSING: readonly TvMissingEpisode[] = [
  {
    season: 1,
    number: 5,
    title: "The Grim Barbarity of Optics and Design",
    plot: "",
    first_aired: "2022-03-11",
    unaired: false,
  },
  {
    season: 2,
    number: 3,
    title: "Who Is Alive?",
    plot: "",
    first_aired: "2025-01-31",
    unaired: false,
  },
  {
    season: 2,
    number: 10,
    title: "Cold Harbor",
    plot: "",
    first_aired: "2099-03-21",
    unaired: true,
  },
];

const VIDEO_STREAMS: DetailVideo["streams"] = [
  {
    kind: "video",
    codec: "HEVC",
    width: 1920,
    height: 1080,
    frame_rate: 23.976,
    bitrate_kbps: 6_200,
    bit_depth: 10,
    sample_rate_hz: 0,
    hdr: [],
    hdr_details: null,
    channels: 0,
    language: "",
    title: "",
    flags: [],
  },
  {
    kind: "audio",
    codec: "E-AC-3",
    width: 0,
    height: 0,
    frame_rate: null,
    bitrate_kbps: 640,
    bit_depth: 0,
    sample_rate_hz: 0,
    hdr: [],
    hdr_details: null,
    channels: 6,
    language: "en",
    title: "",
    flags: ["default"],
  },
  {
    kind: "subtitle",
    codec: "SubRip",
    width: 0,
    height: 0,
    frame_rate: null,
    bitrate_kbps: 0,
    bit_depth: 0,
    sample_rate_hz: 0,
    hdr: [],
    hdr_details: null,
    channels: 0,
    language: "en",
    title: "",
    flags: [],
  },
];

function idOf(index: number): string {
  return String(SYNTHETIC_EPISODE_BASE + index);
}

function resolutionOf(seed: Seed): string {
  return `${seed.height ?? 1080}p`;
}

function rowOf(seed: Seed, index: number, watched: boolean): TvEpisodeRow {
  return {
    id: idOf(index),
    season: seed.season,
    number: seed.number,
    title: seed.title,
    plot: seed.plot,
    first_aired: seed.aired,
    runtime_minutes: 50,
    watched,
    is_new: seed.isNew ?? false,
    thumb: seed.thumb === true ? image(200 + index, BACKDROP_HASH) : null,
    file_name: seed.file ?? "",
    size_bytes: 1_500_000_000,
    resolution: resolutionOf(seed),
    multi_episode: false,
    duplicate: SEEDS.filter((s) => s.season === seed.season && s.number === seed.number).length > 1,
  };
}

const UNCATEGORISED = [
  { id: "150", title: "Some Unnumbered Clip", file_name: "clip.mkv" },
  { id: "151", title: "Severance Interview", file_name: "Severance interview.mkv" },
];

function slot(
  name: string,
  file: string | null,
  n: number,
  hash: string,
  width: number,
  height: number,
) {
  return {
    slot: name,
    file_name: file,
    image: file === null ? null : image(n, hash),
    width: file === null ? 0 : width,
    height: file === null ? 0 : height,
    size_bytes: file === null ? 0 : 900_000,
    online: file === null,
  };
}

function seasons(watchedIds: ReadonlySet<string>): TvSeason[] {
  const rows = SEEDS.map((seed, index) => rowOf(seed, index, watchedIds.has(idOf(index))));
  const make = (number: number, id: string | null, name: string, overview: string): TvSeason => {
    const owned = rows.filter((r) => r.season === number);
    const lacking = MISSING.filter((m) => m.season === number);
    const artwork = ["poster", "fanart", "banner", "thumb"].map((kind, k) =>
      id !== null && number > 0 && kind === "poster"
        ? slot(kind, `season0${number}-poster.jpg`, 300 + number, POSTER_HASH, 1000, 1500)
        : slot(kind, null, 300 + k, "", 0, 0),
    );
    return {
      id,
      number,
      name,
      overview,
      note: "",
      // Like the real reader with the fallback on: the show's poster stands in for a missing one.
      poster: artwork[0]?.image ?? image(2, POSTER_HASH),
      fanart: image(1, BACKDROP_HASH),
      poster_from_show: artwork[0]?.image === null || artwork[0] === undefined,
      artwork,
      first_aired:
        owned
          .map((r) => r.first_aired ?? "")
          .filter((d) => d !== "")
          .sort()[0] ?? null,
      // Like the real reader: one per number, so the second file of an episode adds nothing.
      episodes: episodeCount(owned),
      watched: episodeCount(owned.filter((r) => r.watched)),
      new_episodes: episodeCount(owned.filter((r) => r.is_new)),
      missing: lacking.filter((m) => !m.unaired).length,
      unaired: lacking.filter((m) => m.unaired).length,
    };
  };
  return [
    make(0, null, "", ""),
    make(1, "11", "The Innies", "Mark leads a team at Lumon whose memories are split in two."),
    make(2, "12", "", ""),
  ];
}

export function syntheticShowDetail(
  now: number = Date.now(),
  watchedIds: ReadonlySet<string> = new Set(),
  ordering = "aired",
): ShowDetail {
  const rows = SEEDS.map((seed, index) => rowOf(seed, index, watchedIds.has(idOf(index))));
  const today = new Date(now).toISOString().slice(0, 10);
  const aired = [
    ...rows.map((r) => r.first_aired ?? ""),
    ...MISSING.map((m) => m.first_aired ?? ""),
  ].filter((d) => d !== "");
  return {
    id: SYNTHETIC_SHOW_ID,
    title: "Severance",
    original_title: "",
    tagline: "Work-life balance, taken literally.",
    year: 2022,
    year_end: 0,
    certification: "TV-MA",
    runtime_minutes: 50,
    first_aired: "2022-02-18",
    status: "continuing",
    last_aired:
      aired
        .filter((d) => d <= today)
        .sort()
        .at(-1) ?? null,
    next_aired: aired.filter((d) => d > today).sort()[0] ?? null,
    end_date: null,
    genres: ["Science Fiction", "Drama"],
    studios: ["Apple TV+"],
    countries: ["US"],
    plot: "Mark leads a team at Lumon Industries whose memories have been surgically divided between their work and personal lives.",
    ids: [
      { provider: "tmdb", value: "95396" },
      { provider: "tvdb", value: "371980" },
      { provider: "imdb", value: "tt11280740" },
    ],
    ratings: [
      { source: "imdb", value: 8.7, max: 10, votes: 540_000, fetched_at_ms: 0 },
      { source: "tmdb", value: 8.3, max: 10, votes: 4_200, fetched_at_ms: 0 },
    ],
    rating_chips: [
      { key: "imdb", state: "value", reason: "", provider: null },
      {
        key: "tomatometerallcritics",
        state: "needs_key",
        reason: "OMDb needs an API key.",
        provider: "omdb",
      },
      {
        key: "metacritic",
        state: "needs_key",
        reason: "OMDb needs an API key.",
        provider: "omdb",
      },
      { key: "tmdb", state: "value", reason: "", provider: null },
      {
        key: "trakt",
        state: "connect_service",
        reason: "Connect Trakt to get its rating.",
        provider: "trakt",
      },
    ],
    user_rating: null,
    top250: 0,
    watched: rows.every((r) => r.watched),
    locked: false,
    needs_review: false,
    availability: syntheticAvailability(),
    lifecycle: SHOW_LIFECYCLE,
    is_new: false,
    date_added_ms: now - 5 * DAY_MS,
    folder: FOLDER,
    data_source: "D:\\Shows",
    source_label: "Shows",
    note: "",
    tags: [],
    ordering,
    backdrop: image(1, BACKDROP_HASH),
    logo: null,
    poster: image(2, POSTER_HASH),
    tech: [
      { label: "1080p", facet: "video_format", values: ["1080p"], number: null, notable: false },
      { label: "2160p", facet: "video_format", values: ["2160p"], number: null, notable: false },
      { label: "HEVC", facet: "video_codec", values: ["HEVC"], number: null, notable: false },
      {
        label: "E-AC-3 5.1",
        facet: "audio_codec",
        values: ["E-AC-3"],
        number: null,
        notable: false,
      },
    ],
    original_language: "en",
    credits: [
      {
        name: "Adam Scott",
        role: "Mark Scout",
        kind: "actor",
        portrait: null,
        imdb_id: "nm0779940",
        tmdb_id: "17419",
      },
      {
        name: "Britt Lower",
        role: "Helly R.",
        kind: "actor",
        portrait: null,
        imdb_id: "nm1555340",
        tmdb_id: "1209996",
      },
      {
        name: "Zach Cherry",
        role: "Dylan G.",
        kind: "actor",
        portrait: null,
        imdb_id: null,
        tmdb_id: null,
      },
      {
        name: "Ben Stiller",
        role: "",
        kind: "director",
        portrait: null,
        imdb_id: "nm0001774",
        tmdb_id: "7399",
      },
      {
        name: "Dan Erickson",
        role: "Creator",
        kind: "writer",
        portrait: null,
        imdb_id: null,
        tmdb_id: null,
      },
    ],
    guests: [
      { name: "Michael Chernus", role: "Ricken", episodes: 3 },
      { name: "Dichen Lachman", role: "Ms. Casey", episodes: 2 },
    ],
    seasons: seasons(watchedIds),
    episodes: rows,
    missing: [...MISSING],
    episode_list: true,
    uncategorised: UNCATEGORISED.map((u) => ({
      id: u.id,
      title: u.title,
      file_name: u.file_name,
      folder: FOLDER,
      size_bytes: 480_000_000,
    })),
    files: [
      { kind: "nfo", name: "tvshow.nfo", size_bytes: 4_200 },
      { kind: "poster", name: "poster.jpg", size_bytes: 1_100_000 },
      { kind: "fanart", name: "fanart.jpg", size_bytes: 2_400_000 },
      { kind: "theme", name: "theme.mp3", size_bytes: 3_100_000 },
    ],
    artwork: [
      slot("poster", "poster.jpg", 2, POSTER_HASH, 2000, 3000),
      slot("fanart", "fanart.jpg", 1, BACKDROP_HASH, 3840, 2160),
      slot("clearlogo", "clearlogo.png", 3, LOGO_HASH, 800, 310),
      slot("thumb", null, 0, "", 0, 0),
      slot("keyart", null, 0, "", 0, 0),
      slot("banner", "banner.jpg", 4, BACKDROP_HASH, 1000, 185),
      slot("clearart", null, 0, "", 0, 0),
      slot("disc", null, 0, "", 0, 0),
    ],
    trailers: [
      {
        name: "Official Trailer",
        provider: "tmdb",
        quality: "1080p",
        language: "en",
        url: "https://example.test/severance-trailer",
        local_file: null,
      },
    ],
    new_episodes: rows.filter((r) => r.is_new).length,
    duplicate_episodes: new Set(
      rows.filter((r) => r.duplicate).map((r) => `${r.season}x${r.number}`),
    ).size,
    watched_episodes: rows.filter((r) => r.watched).length,
  };
}

export function syntheticEpisodeDetail(
  id: string,
  watchedIds: ReadonlySet<string> = new Set(),
  now: number = Date.now(),
): EpisodeDetail {
  const index = Number(id) - SYNTHETIC_EPISODE_BASE;
  const seed = SEEDS[index];
  const loose = UNCATEGORISED.find((u) => u.id === id);
  if (seed === undefined && loose === undefined) {
    throw new DetailRequestError("not_found", "That episode is not in the library any more.");
  }
  const row = seed === undefined ? null : rowOf(seed, index, watchedIds.has(id));
  const season = row?.season ?? 0;
  const number = row?.number ?? 0;
  const ordered = SEEDS.map((s, i) => ({ s, i })).sort(
    (a, b) => a.s.season - b.s.season || a.s.number - b.s.number,
  );
  const position = ordered.findIndex(({ i }) => i === index);
  const link = (at: number) => {
    const entry = ordered[at];
    return at < 0 || entry === undefined
      ? null
      : {
          id: idOf(entry.i),
          season: entry.s.season,
          number: entry.s.number,
          title: entry.s.title,
        };
  };
  const file = seed?.file ?? loose?.file_name ?? "";
  const resolution = seed === undefined ? "720p" : resolutionOf(seed);
  const video: DetailVideo = {
    name: file,
    folder: season === 0 ? FOLDER : SEASON_ONE_FOLDER,
    size_bytes: 1_500_000_000,
    container: "Matroska",
    duration_secs: 50 * 60,
    analysed: true,
    read_problem: "",
    streams: VIDEO_STREAMS,
  };
  return {
    id,
    show_id: SYNTHETIC_SHOW_ID,
    show_title: "Severance",
    season_id: season === 1 ? "11" : season === 2 ? "12" : null,
    season,
    number,
    title: row?.title ?? loose?.title ?? "",
    original_title: "",
    plot: row?.plot ?? "",
    first_aired: row?.first_aired ?? null,
    runtime_minutes: row === null ? 0 : 50,
    ids: row === null ? [] : [{ provider: "tvdb", value: String(8_000_000 + index) }],
    ratings:
      row === null ? [] : [{ source: "tmdb", value: 8.4, max: 10, votes: 310, fetched_at_ms: 0 }],
    user_rating: null,
    watched: row?.watched ?? false,
    play_count: row?.watched === true ? 1 : 0,
    locked: false,
    availability: syntheticAvailability(),
    lifecycle: SHOW_LIFECYCLE,
    is_new: row?.is_new ?? false,
    multi_episode: false,
    duplicate: row?.duplicate ?? false,
    uncategorised: row === null,
    date_added_ms: now - 2 * DAY_MS,
    folder: video.folder,
    data_source: "D:\\Shows",
    source_label: "Shows",
    note: "",
    tags: [],
    numbers:
      row === null
        ? []
        : [
            { ordering: "aired", name: "", season, episode: number },
            { ordering: "dvd", name: "", season, episode: number },
          ],
    directors: row === null ? [] : ["Ben Stiller"],
    writers: row === null ? [] : ["Dan Erickson"],
    studios: [],
    guests:
      row?.number === 1 && season === 1
        ? [
            {
              name: "Michael Chernus",
              role: "Ricken",
              kind: "guest",
              portrait: null,
              imdb_id: null,
              tmdb_id: null,
            },
          ]
        : [],
    credits:
      row === null
        ? []
        : [
            {
              name: "Ben Stiller",
              role: "",
              kind: "director",
              portrait: null,
              imdb_id: "nm0001774",
              tmdb_id: "7399",
            },
            {
              name: "Dan Erickson",
              role: "Teleplay",
              kind: "writer",
              portrait: null,
              imdb_id: null,
              tmdb_id: null,
            },
          ],
    thumb: row?.thumb ?? null,
    show_backdrop: image(1, BACKDROP_HASH),
    tech: [
      {
        label: resolution,
        facet: "video_format",
        values: [resolution],
        number: null,
        notable: false,
      },
      ...(row === null
        ? []
        : [
            {
              label: "E-AC-3 5.1",
              facet: "audio_codec",
              values: ["E-AC-3"],
              number: null,
              notable: false,
            },
          ]),
    ],
    original_language: "en",
    hero_audio:
      row === null
        ? null
        : {
            index: 0,
            label: "E-AC-3 5.1",
            codec: "E-AC-3",
            channels: 6,
            language: "en",
            title: "",
            flags: ["default"],
          },
    videos: file === "" ? [] : [video],
    files: [{ kind: "nfo", name: file.replace(/\.[^.]+$/, ".nfo"), size_bytes: 3_000 }],
    artwork: [
      slot("thumb", row?.thumb ? "thumb.jpg" : null, 200 + index, BACKDROP_HASH, 1920, 1080),
    ],
    subtitles: [
      {
        file_name: null,
        language: "en",
        codec: "SubRip",
        title: "",
        flags: [],
        video: file === "" ? null : file,
        position: 0,
        codec_id: "S_TEXT/UTF8",
      },
    ],
    previous: row === null ? null : link(position - 1),
    next: row === null ? null : link(position + 1),
  };
}

function syntheticShowNfo(): NfoPreview {
  return {
    text: [
      '<?xml version="1.0" encoding="utf-8" standalone="yes"?>',
      "<!--created by CineVault for Kodi-->",
      "<tvshow>",
      "  <title>Severance</title>",
      '  <uniqueid type="tvdb" default="true">371980</uniqueid>',
      "  <premiered>2022-02-18</premiered>",
      "  <status>Continuing</status>",
      "</tvshow>",
    ].join("\n"),
    format: "Kodi",
    path: `${FOLDER}\\tvshow.nfo`,
    sync: "in_sync",
    file_text: null,
  };
}

function syntheticEpisodeNfo(): NfoPreview {
  return {
    text: [
      '<?xml version="1.0" encoding="utf-8" standalone="yes"?>',
      "<!--created by CineVault for Kodi-->",
      "<episodedetails>",
      "  <title>Half Loop</title>",
      "  <season>1</season>",
      "  <episode>2</episode>",
      "  <aired>2022-02-18</aired>",
      "</episodedetails>",
    ].join("\n"),
    format: "Kodi",
    path: `${SEASON_ONE_FOLDER}\\Severance - S01E02 - Half Loop.nfo`,
    sync: "differs",
    file_text: null,
  };
}

/** A backend over the fixture show. Watched changes are kept for the session. */
const SYNTHETIC_ORDERINGS: readonly ShowOrdering[] = [
  { ordering: "aired", name: "", label: "Aired order" },
  { ordering: "dvd", name: "", label: "DVD order" },
  { ordering: "absolute", name: "", label: "Absolute order" },
];

export function createSyntheticTvBackend(): TvBackend {
  const watched = new Set<string>();
  let active: ShowOrdering = SYNTHETIC_ORDERINGS[0] as ShowOrdering;
  const checkShow = (id: string): void => {
    if (id !== SYNTHETIC_SHOW_ID) {
      throw new DetailRequestError("not_found", "That show is not in the library any more.");
    }
  };
  return {
    show: (id) =>
      Promise.resolve().then(() => {
        checkShow(id);
        return syntheticShowDetail(Date.now(), watched, active.ordering);
      }),
    episode: (id) => Promise.resolve().then(() => syntheticEpisodeDetail(id, watched)),
    showNfo: (id) =>
      Promise.resolve().then(() => {
        checkShow(id);
        return syntheticShowNfo();
      }),
    episodeNfo: (id) =>
      Promise.resolve().then(() => {
        syntheticEpisodeDetail(id, watched);
        return syntheticEpisodeNfo();
      }),
    setWatched: (ids, next) =>
      Promise.resolve().then(() => {
        const previous = [];
        for (const id of ids) {
          const index = Number(id) - SYNTHETIC_EPISODE_BASE;
          if (SEEDS[index] === undefined) continue;
          const was = watched.has(id);
          if (was === next) continue;
          previous.push({ id, watched: was, play_count: was ? 1 : 0, last_watched_ms: null });
          if (next) watched.add(id);
          else watched.delete(id);
        }
        return previous;
      }),
    restoreWatched: (previous) =>
      Promise.resolve().then(() => {
        for (const state of previous) {
          if (state.watched) watched.add(state.id);
          else watched.delete(state.id);
        }
      }),
    orderings: (id) =>
      Promise.resolve().then(() => {
        checkShow(id);
        return { active, options: [...SYNTHETIC_ORDERINGS] };
      }),
    setOrdering: (id, ordering) =>
      Promise.resolve().then(() => {
        checkShow(id);
        const chosen = SYNTHETIC_ORDERINGS.find((o) => o.ordering === ordering.ordering);
        if (chosen === undefined) {
          throw new DetailRequestError(
            "ordering_not_available",
            `${ordering.name === "" ? ordering.ordering : ordering.name} is not available for this show.`,
          );
        }
        active = chosen;
        return {
          ordering: chosen,
          matched: SEEDS.length,
          unmatched: 0,
          locked: 0,
          missing: 0,
          seasons: [1, 2],
        };
      }),
    searchEpisodes: (id, query) =>
      Promise.resolve().then(() => {
        checkShow(id);
        return syntheticEpisodeHits(query);
      }),
    assignEpisode: (_episodeId, pick) =>
      Promise.resolve().then((): AssignedEpisode => {
        if (pick.season === null || pick.episode === null) {
          throw new DetailRequestError(
            "stale_pick",
            "That episode is no longer in the provider's list. Search again.",
          );
        }
        return {
          season: pick.season,
          episode: pick.episode,
          write: { written: ["title", "plot"], locked_fields: [], kept: [], item_locked: false },
        };
      }),
    scrapeShowEpisodes: (id) =>
      Promise.resolve().then(() => {
        checkShow(id);
        return {
          matched: SEEDS.length,
          changed: SEEDS.length,
          unmatched: 0,
          locked: 0,
          field_locked: 0,
          missing: 0,
        };
      }),
    scrapeEpisode: (episodeId) =>
      Promise.resolve().then(() => {
        const seed = SEEDS[Number(episodeId) - SYNTHETIC_EPISODE_BASE];
        if (seed === undefined) {
          throw new DetailRequestError(
            "no_match",
            "The provider's list has nothing for this episode. Search it by title.",
          );
        }
        return { written: ["title", "plot"], locked_fields: [], kept: [], item_locked: false };
      }),
  };
}

/** The titles of the fixture show that contain the typed text, as a provider's list would give them. */
function syntheticEpisodeHits(query: string): EpisodeHit[] {
  const wanted = query.trim().toLowerCase();
  if (wanted === "") return [];
  return SEEDS.filter((seed) => seed.title.toLowerCase().includes(wanted)).map((seed) => ({
    title: seed.title,
    season: seed.season,
    episode: seed.number,
    first_aired: seed.aired,
    plot: seed.plot,
    score: 0.9,
    pick: {
      ids: [{ provider: "tvdb", value: `${seed.season}x${seed.number}` }],
      season: seed.season,
      episode: seed.number,
    },
  }));
}
