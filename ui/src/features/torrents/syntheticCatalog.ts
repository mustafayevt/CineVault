import type {
  TorrentEpisodeRefDto,
  TorrentLandingTitleDto,
  TorrentLibraryCopyDto,
  TorrentReleaseDto,
} from "../../bindings";
import type { TitleKind } from "./torrentsSearch";

/**
 * The titles and releases of the synthetic Torrent search backend (`bun run dev`, `bun run shots`
 * and the tests): the titles of the approved redesign (Blade Runner 2049 in the library at 1080p,
 * Severance with a season and a half held), some that share a name, and three that show a state:
 * every source failing, sources that never finish, and nothing found. No network.
 */

export interface CatalogTitle {
  kind: TitleKind;
  tmdbId: number;
  title: string;
  original: string | null;
  year: number;
  overview: string;
  runtime: number;
  imdb: string;
  anime: boolean;
  certification: string | null;
  rating: number | null;
  genres: readonly string[];
  /** TMDB's vote count, for the order of titles of one name. */
  votes: number;
  /** How the library lists it, when it holds it. */
  inLibrary: string | null;
  /** The movie's copy in the library. */
  copy?: TorrentLibraryCopyDto;
  /** The episodes of a show the library holds. */
  held?: readonly TorrentEpisodeRefDto[];
  /** When the library added it, days before the fixtures' "now". */
  addedDaysAgo?: number;
  /** What the release search does: answer (default), fail everywhere, never finish, find nothing. */
  behaviour?: "normal" | "failing" | "slow" | "empty";
}

const BLADE_RUNNER_PLOT =
  "Thirty years after the events of the first film, a new blade runner, LAPD Officer K, unearths a long-buried secret that has the potential to plunge what's left of society into chaos. His discovery leads him on a quest to find Rick Deckard, a former blade runner who has been missing for thirty years.";

const SEVERANCE_PLOT =
  "Mark leads a team of office workers whose memories have been surgically divided between their work and personal lives. When a mysterious colleague appears outside of work, it begins a journey to discover the truth about their jobs.";

const GB = 1_000_000_000;

export const CATALOG: readonly CatalogTitle[] = [
  {
    kind: "movie",
    tmdbId: 1,
    title: "Blade Runner 2049",
    original: null,
    year: 2017,
    overview: BLADE_RUNNER_PLOT,
    runtime: 164,
    imdb: "tt1856101",
    anime: false,
    certification: "R",
    rating: 7.5,
    genres: ["Science Fiction", "Drama"],
    votes: 14_000,
    inLibrary: "Blade Runner 2049 (2017)",
    copy: {
      resolution: "1080p",
      source: "WEB-DL",
      hdr: null,
      size: 5.6 * GB,
      fileName: "Blade.Runner.2049.2017.1080p.WEB-DL.DD5.1.H264-FGT.mkv",
    },
    addedDaysAgo: 40,
  },
  {
    kind: "movie",
    tmdbId: 2,
    title: "Dune",
    original: null,
    year: 2021,
    overview: "Paul Atreides travels to the most dangerous planet in the universe.",
    runtime: 155,
    imdb: "tt1160419",
    anime: false,
    certification: "PG-13",
    rating: 7.8,
    genres: ["Science Fiction", "Adventure"],
    votes: 13_000,
    inLibrary: null,
  },
  {
    kind: "movie",
    tmdbId: 3,
    title: "Heat",
    original: null,
    year: 1995,
    overview:
      "Obsessive master thief Neil McCauley leads a top-notch crew on various daring heists throughout Los Angeles while determined detective Vincent Hanna pursues him without rest.",
    runtime: 170,
    imdb: "tt0113277",
    anime: false,
    certification: "R",
    rating: 7.9,
    genres: ["Crime", "Drama", "Action"],
    votes: 7_400,
    inLibrary: "Heat (1995)",
    copy: {
      resolution: "720p",
      source: "BluRay",
      hdr: null,
      size: 4.4 * GB,
      fileName: "Heat.1995.720p.BluRay.x264-SiNNERS.mkv",
    },
    addedDaysAgo: 120,
  },
  {
    kind: "movie",
    tmdbId: 4,
    title: "Blade Runner",
    original: null,
    year: 1982,
    overview: "A blade runner must pursue and terminate four replicants who stole a ship in space.",
    runtime: 117,
    imdb: "tt0083658",
    anime: false,
    certification: "R",
    rating: 7.9,
    genres: ["Science Fiction", "Drama", "Thriller"],
    votes: 14_500,
    inLibrary: null,
  },
  {
    kind: "movie",
    tmdbId: 5,
    title: "Blade",
    original: null,
    year: 1998,
    overview: "A half-vampire, half-mortal man becomes a protector of the mortal race.",
    runtime: 120,
    imdb: "tt0120611",
    anime: false,
    certification: "R",
    rating: 6.8,
    genres: ["Horror", "Action"],
    votes: 6_900,
    inLibrary: null,
  },
  {
    kind: "movie",
    tmdbId: 6,
    title: "The Thing",
    original: null,
    year: 1982,
    overview:
      "In remote Antarctica, a group of American research scientists are disturbed by a shape-shifter.",
    runtime: 109,
    imdb: "tt0084787",
    anime: false,
    certification: "R",
    rating: 8.1,
    genres: ["Horror", "Mystery", "Science Fiction"],
    votes: 7_300,
    inLibrary: "The Thing (1982)",
    copy: {
      resolution: "1080p",
      source: "BluRay",
      hdr: null,
      size: 9.1 * GB,
      fileName: "The.Thing.1982.1080p.BluRay.x264-AMIABLE.mkv",
    },
    addedDaysAgo: 200,
  },
  {
    kind: "movie",
    tmdbId: 7,
    title: "Oppenheimer",
    original: null,
    year: 2023,
    overview: "The story of J. Robert Oppenheimer's role in the development of the atomic bomb.",
    runtime: 181,
    imdb: "tt15398776",
    anime: false,
    certification: "R",
    rating: 8.1,
    genres: ["Drama", "History"],
    votes: 9_800,
    inLibrary: "Oppenheimer (2023)",
    copy: {
      resolution: "1080p",
      source: "WEB-DL",
      hdr: null,
      size: 7.2 * GB,
      fileName: "Oppenheimer.2023.1080p.WEB-DL.DDP5.1.H.264-FLUX.mkv",
    },
    addedDaysAgo: 90,
  },
  {
    kind: "movie",
    tmdbId: 8,
    title: "Alien",
    original: null,
    year: 1979,
    overview:
      "During its return to the earth, commercial spaceship Nostromo intercepts a distress signal.",
    runtime: 117,
    imdb: "tt0078748",
    anime: false,
    certification: "R",
    rating: 8.2,
    genres: ["Horror", "Science Fiction"],
    votes: 15_000,
    inLibrary: "Alien (1979)",
    copy: {
      resolution: "720p",
      source: "BluRay",
      hdr: null,
      size: 4.0 * GB,
      fileName: "Alien.1979.720p.BluRay.x264-GRP.mkv",
    },
    addedDaysAgo: 300,
  },
  {
    kind: "movie",
    tmdbId: 9,
    title: "Dune: Part Two",
    original: null,
    year: 2024,
    overview: "Paul Atreides unites with Chani and the Fremen while on a path of revenge.",
    runtime: 167,
    imdb: "tt15239678",
    anime: false,
    certification: "PG-13",
    rating: 8.2,
    genres: ["Science Fiction", "Adventure"],
    votes: 6_500,
    inLibrary: "Dune: Part Two (2024)",
    copy: {
      resolution: "2160p",
      source: "WEB-DL",
      hdr: "DV",
      size: 24.1 * GB,
      fileName: "Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR.H.265-FLUX.mkv",
    },
    addedDaysAgo: 0,
  },
  {
    kind: "movie",
    tmdbId: 10,
    title: "Poor Things",
    original: null,
    year: 2023,
    overview: "Brought back to life by an unorthodox scientist, a young woman runs off.",
    runtime: 141,
    imdb: "tt14230458",
    anime: false,
    certification: "R",
    rating: 7.7,
    genres: ["Science Fiction", "Romance", "Comedy"],
    votes: 4_100,
    inLibrary: "Poor Things (2023)",
    copy: {
      resolution: "1080p",
      source: "WEB-DL",
      hdr: null,
      size: 6.0 * GB,
      fileName: "Poor.Things.2023.1080p.WEB-DL.mkv",
    },
    addedDaysAgo: 2,
  },
  {
    kind: "movie",
    tmdbId: 11,
    title: "Past Lives",
    original: null,
    year: 2023,
    overview: "Nora and Hae Sung, two deeply connected childhood friends, are wrested apart.",
    runtime: 106,
    imdb: "tt13238346",
    anime: false,
    certification: "PG-13",
    rating: 7.8,
    genres: ["Drama", "Romance"],
    votes: 1_900,
    inLibrary: "Past Lives (2023)",
    copy: {
      resolution: "1080p",
      source: "BluRay",
      hdr: null,
      size: 8.3 * GB,
      fileName: "Past.Lives.2023.1080p.BluRay.x264-GRP.mkv",
    },
    addedDaysAgo: 3,
  },
  {
    kind: "movie",
    tmdbId: 90,
    title: "The Quiet Archive",
    original: null,
    year: 2019,
    overview: "A librarian finds a reel no catalogue lists. (Every source fails for this title.)",
    runtime: 98,
    imdb: "tt0000090",
    anime: false,
    certification: null,
    rating: 6.4,
    genres: ["Mystery"],
    votes: 120,
    inLibrary: null,
    behaviour: "failing",
  },
  {
    kind: "movie",
    tmdbId: 91,
    title: "Midnight Signal",
    original: null,
    year: 2021,
    overview:
      "A night-shift operator hears a voice on a dead channel. (Some sources never finish.)",
    runtime: 104,
    imdb: "tt0000091",
    anime: false,
    certification: "PG-13",
    rating: 6.9,
    genres: ["Thriller"],
    votes: 300,
    inLibrary: null,
    behaviour: "slow",
  },
  {
    kind: "movie",
    tmdbId: 92,
    title: "Empty Reel",
    original: null,
    year: 2020,
    overview: "A projectionist runs a film nobody made. (Nothing is found for this title.)",
    runtime: 88,
    imdb: "tt0000092",
    anime: false,
    certification: null,
    rating: null,
    genres: ["Drama"],
    votes: 40,
    inLibrary: null,
    behaviour: "empty",
  },
  {
    kind: "show",
    tmdbId: 1,
    title: "Severance",
    original: null,
    year: 2022,
    overview: SEVERANCE_PLOT,
    runtime: 50,
    imdb: "tt11280740",
    anime: false,
    certification: "TV-MA",
    rating: 8.4,
    genres: ["Drama", "Mystery", "Science Fiction"],
    votes: 2_600,
    inLibrary: "Severance (2022)",
    held: [
      ...Array.from({ length: 9 }, (_, index) => ({ season: 1, episode: index + 1 })),
      { season: 2, episode: 1 },
      { season: 2, episode: 2 },
      { season: 2, episode: 3 },
    ],
    addedDaysAgo: 1,
  },
  {
    kind: "show",
    tmdbId: 2,
    title: "Blade Runner: Black Lotus",
    original: null,
    year: 2021,
    overview: "A young woman wakes up in Los Angeles with no memory, armed with deadly skills.",
    runtime: 25,
    imdb: "tt9686194",
    anime: true,
    certification: "TV-14",
    rating: 7.3,
    genres: ["Animation", "Science Fiction"],
    votes: 400,
    inLibrary: null,
  },
  {
    kind: "show",
    tmdbId: 3,
    title: "HEAT",
    original: null,
    year: 2015,
    overview: "A reality series about a fire brigade.",
    runtime: 45,
    imdb: "tt0000093",
    anime: false,
    certification: null,
    rating: 5.1,
    genres: ["Reality"],
    votes: 12,
    inLibrary: null,
  },
  {
    kind: "show",
    tmdbId: 4,
    title: "Shōgun",
    original: null,
    year: 2024,
    overview: "In Japan in the year 1600, a mysterious European ship is found marooned.",
    runtime: 60,
    imdb: "tt2788316",
    anime: false,
    certification: "TV-MA",
    rating: 8.6,
    genres: ["Drama", "War & Politics"],
    votes: 1_200,
    inLibrary: "Shōgun (2024)",
    held: [{ season: 1, episode: 1 }],
    addedDaysAgo: 1,
  },
];

/** The episodes of the shows, by show and season. */
export const SHOW_EPISODES: Readonly<
  Record<number, Readonly<Record<number, readonly { name: string; aired: string }[]>>>
> = {
  1: {
    1: [
      { name: "Good News About Hell", aired: "2022-02-18" },
      { name: "Half Loop", aired: "2022-02-18" },
      { name: "In Perpetuity", aired: "2022-02-25" },
      { name: "The You You Are", aired: "2022-03-04" },
      { name: "The Grim Barbarity of Optics and Design", aired: "2022-03-11" },
      { name: "Hide and Seek", aired: "2022-03-18" },
      { name: "Defiant Jazz", aired: "2022-03-25" },
      { name: "What's for Dinner?", aired: "2022-04-01" },
      { name: "The We We Are", aired: "2022-04-08" },
    ],
    2: [
      { name: "Hello, Ms. Cobel", aired: "2025-01-17" },
      { name: "Goodbye, Mrs. Selvig", aired: "2025-01-24" },
      { name: "Who Is Alive?", aired: "2025-01-31" },
      { name: "Woe's Hollow", aired: "2025-02-07" },
      { name: "Trojan's Horse", aired: "2025-02-14" },
      { name: "Attila", aired: "2025-02-21" },
      { name: "Chikhai Bardo", aired: "2025-02-28" },
      { name: "Sweet Vitriol", aired: "2025-03-07" },
      { name: "The After Hours", aired: "2025-03-14" },
      { name: "Cold Harbor", aired: "2025-03-21" },
    ],
  },
  2: {
    1: Array.from({ length: 13 }, (_, i) => ({ name: `Episode ${i + 1}`, aired: "2021-11-14" })),
  },
  3: {
    1: Array.from({ length: 6 }, (_, i) => ({ name: `Episode ${i + 1}`, aired: "2015-05-01" })),
  },
  4: {
    1: Array.from({ length: 10 }, (_, i) => ({ name: `Episode ${i + 1}`, aired: "2024-02-27" })),
  },
};

/**
 * The IDs the synthetic library pages carry for their titles (the Dune movie page and the
 * Severance show page), mapped to the catalogue entries, so "Find releases…" works in a browser.
 */
const ALIASES: Readonly<Record<string, number>> = { "movie:693134": 2, "show:95396": 1 };

export function catalogTitle(kind: TitleKind, tmdbId: number): CatalogTitle | undefined {
  const id = ALIASES[`${kind}:${tmdbId}`] ?? tmdbId;
  return CATALOG.find((entry) => entry.kind === kind && entry.tmdbId === id);
}

// ---------------------------------------------------------------- sources

export interface SyntheticSource {
  id: string;
  name: string;
  kind: "built-in" | "indexer";
  movies: boolean;
  tv: boolean;
  /** Milliseconds before it answers, at speed 1. */
  delay: number;
}

export const SOURCES: readonly SyntheticSource[] = [
  { id: "torrentio", name: "Torrentio", kind: "built-in", movies: true, tv: true, delay: 150 },
  { id: "yts", name: "YTS", kind: "built-in", movies: true, tv: false, delay: 300 },
  { id: "eztv", name: "EZTV", kind: "built-in", movies: false, tv: true, delay: 300 },
  { id: "apibay", name: "The Pirate Bay", kind: "built-in", movies: true, tv: true, delay: 900 },
  { id: "knaben", name: "Knaben", kind: "built-in", movies: true, tv: true, delay: 600 },
  { id: "nyaa", name: "Nyaa", kind: "built-in", movies: false, tv: false, delay: 300 },
  { id: "idx-1337x", name: "1337x (indexer)", kind: "indexer", movies: true, tv: true, delay: 700 },
  {
    id: "idx-limetorrents",
    name: "LimeTorrents (indexer)",
    kind: "indexer",
    movies: true,
    tv: true,
    delay: 800,
  },
];

function slug(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

/** The page each source gives for a release, as the live sources build them. */
function pageOf(source: string, entry: CatalogTitle, number: number): string | null {
  switch (source) {
    case "yts":
      return `https://yts.mx/movies/${slug(`${entry.title} ${entry.year}`)}`;
    case "apibay":
      return `https://thepiratebay.org/description.php?id=${25_118_000 + number}`;
    case "knaben":
      return `https://knaben.org/torrent/1337x/${4_418_000 + number}`;
    case "eztv":
      return `https://eztv.re/ep/${1_883_000 + number}/${slug(entry.title)}/`;
    case "idx-1337x":
      return `https://1337x.to/torrent/${4_418_000 + number}/`;
    case "idx-limetorrents":
      return `https://www.limetorrents.lol/torrent/${(0x4a1d00 + number).toString(16).toUpperCase()}.html`;
    default:
      return null;
  }
}

// ---------------------------------------------------------------- releases

interface Row {
  /** The name after the title and year: `2160p.UHD.BluRay.REMUX...-FGT`. */
  tail: string;
  resolution: "2160p" | "1080p" | "720p" | "480p";
  source: string | null;
  codec: string | null;
  hdr: string | null;
  audio: string | null;
  gb: number;
  seeders: number;
  leechers: number;
  ageDays: number;
  from: readonly string[];
  torrent?: boolean;
  flags?: readonly { kind: string; text: string }[];
  upgrade?: "better" | "same" | "worse";
  /** A release of another title (the name says so). */
  other?: string;
  pack?: "season" | "seasons" | "complete" | "episodes";
}

const MOVIE_ROWS: readonly Row[] = [
  {
    tail: "2160p.UHD.BluRay.REMUX.DV.HDR.HEVC.TrueHD.Atmos.7.1-FGT",
    resolution: "2160p",
    source: "Remux",
    codec: "HEVC",
    hdr: "DV",
    audio: "TrueHD Atmos 7.1",
    gb: 61.2,
    seeders: 214,
    leechers: 12,
    ageDays: 9,
    from: ["torrentio", "apibay", "knaben", "idx-1337x"],
    upgrade: "better",
  },
  {
    tail: "2160p.BluRay.x265.10bit.HDR.TrueHD.7.1.Atmos-SWTYBLZ",
    resolution: "2160p",
    source: "BluRay",
    codec: "HEVC",
    hdr: "HDR10",
    audio: "TrueHD Atmos 7.1",
    gb: 38.4,
    seeders: 167,
    leechers: 9,
    ageDays: 31,
    from: ["torrentio", "yts", "knaben"],
    torrent: true,
    upgrade: "better",
  },
  {
    tail: "2160p.WEB-DL.DDP5.1.HDR.HEVC-CMRG",
    resolution: "2160p",
    source: "WEB-DL",
    codec: "HEVC",
    hdr: "HDR10+",
    audio: "DD+ 5.1",
    gb: 17.9,
    seeders: 142,
    leechers: 20,
    ageDays: 3,
    from: ["torrentio", "apibay"],
    upgrade: "better",
  },
  {
    tail: "2160p.WEBRip.x265.AAC-YTS",
    resolution: "2160p",
    source: "WEBRip",
    codec: "HEVC",
    hdr: null,
    audio: "AAC 2.0",
    gb: 6.1,
    seeders: 0,
    leechers: 2,
    ageDays: 420,
    from: ["yts"],
    upgrade: "same",
    flags: [
      { kind: "no-seeders", text: "No seeders" },
      { kind: "too-small", text: "Small for 2160p" },
    ],
  },
  {
    tail: "1080p.BluRay.REMUX.AVC.TrueHD.Atmos.7.1-FGT",
    resolution: "1080p",
    source: "Remux",
    codec: "AVC",
    hdr: null,
    audio: "TrueHD Atmos 7.1",
    gb: 34.7,
    seeders: 96,
    leechers: 4,
    ageDays: 760,
    from: ["torrentio", "knaben", "idx-limetorrents"],
    upgrade: "better",
  },
  {
    tail: "1080p.BluRay.x264.DTS-HD.MA.7.1-FGT",
    resolution: "1080p",
    source: "BluRay",
    codec: "AVC",
    hdr: null,
    audio: "DTS-HD 7.1",
    gb: 14.7,
    seeders: 301,
    leechers: 24,
    ageDays: 120,
    from: ["torrentio", "yts", "apibay", "knaben"],
    torrent: true,
    upgrade: "better",
  },
  {
    tail: "1080p.WEB-DL.DD5.1.H264-FGT",
    resolution: "1080p",
    source: "WEB-DL",
    codec: "AVC",
    hdr: null,
    audio: "AC3 5.1",
    gb: 5.6,
    seeders: 288,
    leechers: 15,
    ageDays: 45,
    from: ["torrentio", "knaben"],
    upgrade: "same",
  },
  {
    tail: "[1080p].[YTS.AM]",
    resolution: "1080p",
    source: "BluRay",
    codec: "AVC",
    hdr: null,
    audio: "AAC 2.0",
    gb: 2.2,
    seeders: 1190,
    leechers: 48,
    ageDays: 22,
    from: ["yts", "apibay"],
    torrent: true,
    upgrade: "worse",
  },
  {
    tail: "720p.BluRay.x264-SPARKS",
    resolution: "720p",
    source: "BluRay",
    codec: "AVC",
    hdr: null,
    audio: "AC3 5.1",
    gb: 6.6,
    seeders: 44,
    leechers: 3,
    ageDays: 1900,
    from: ["apibay", "knaben"],
    upgrade: "worse",
  },
  {
    tail: "HDCAM.x264.AAC-ETRG",
    resolution: "720p",
    source: "CAM",
    codec: "AVC",
    hdr: null,
    audio: "AAC 2.0",
    gb: 1.4,
    seeders: 3,
    leechers: 1,
    ageDays: 2900,
    from: ["apibay"],
    upgrade: "worse",
    flags: [{ kind: "cam", text: "Camera recording" }],
  },
  {
    tail: "1080p.BluRay.x264-AMIABLE",
    resolution: "1080p",
    source: "BluRay",
    codec: "AVC",
    hdr: null,
    audio: "DTS 5.1",
    gb: 10.9,
    seeders: 210,
    leechers: 11,
    ageDays: 1500,
    from: ["torrentio", "apibay"],
    other: "Part.Two",
  },
  {
    tail: "2160p.WEB-DL.DDP5.1.HEVC-NTb",
    resolution: "2160p",
    source: "WEB-DL",
    codec: "HEVC",
    hdr: null,
    audio: "DD+ 5.1",
    gb: 15.3,
    seeders: 58,
    leechers: 6,
    ageDays: 300,
    from: ["knaben"],
    other: "Making.Of",
  },
];

const PACK_ROWS: readonly Row[] = [
  {
    tail: "2160p.ATVP.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX",
    resolution: "2160p",
    source: "WEB-DL",
    codec: "HEVC",
    hdr: "DV",
    audio: "DD+ Atmos 5.1",
    gb: 68,
    seeders: 128,
    leechers: 31,
    ageDays: 6,
    from: ["torrentio", "knaben", "idx-1337x"],
    pack: "season",
  },
  {
    tail: "1080p.ATVP.WEB-DL.DDP5.1.H.264-FLUX",
    resolution: "1080p",
    source: "WEB-DL",
    codec: "AVC",
    hdr: null,
    audio: "DD+ 5.1",
    gb: 31.8,
    seeders: 402,
    leechers: 60,
    ageDays: 6,
    from: ["torrentio", "eztv", "apibay", "knaben"],
    upgrade: "same",
    pack: "season",
  },
  {
    tail: "COMPLETE.1080p.WEB.H264-MIXED",
    resolution: "1080p",
    source: "WEB-DL",
    codec: "AVC",
    hdr: null,
    audio: "DD+ 5.1",
    gb: 52.4,
    seeders: 61,
    leechers: 9,
    ageDays: 2,
    from: ["apibay"],
    upgrade: "same",
    pack: "seasons",
  },
];

const EPISODE_ROWS: readonly Row[] = [
  {
    tail: "2160p.ATVP.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX",
    resolution: "2160p",
    source: "WEB-DL",
    codec: "HEVC",
    hdr: "DV",
    audio: "DD+ Atmos 5.1",
    gb: 6.9,
    seeders: 96,
    leechers: 8,
    ageDays: 6,
    from: ["torrentio", "eztv", "knaben"],
    upgrade: "better",
  },
  {
    tail: "1080p.ATVP.WEB-DL.DDP5.1.H.264-FLUX",
    resolution: "1080p",
    source: "WEB-DL",
    codec: "AVC",
    hdr: null,
    audio: "DD+ 5.1",
    gb: 3.1,
    seeders: 310,
    leechers: 22,
    ageDays: 6,
    from: ["torrentio", "eztv", "apibay", "knaben"],
    torrent: true,
    upgrade: "same",
  },
  {
    tail: "1080p.WEB.H264-ETHEL",
    resolution: "1080p",
    source: "WEBRip",
    codec: "AVC",
    hdr: null,
    audio: "DD+ 5.1",
    gb: 2.4,
    seeders: 455,
    leechers: 40,
    ageDays: 6,
    from: ["eztv", "apibay"],
    torrent: true,
    upgrade: "same",
  },
  {
    tail: "720p.WEB.x265-MiNX",
    resolution: "720p",
    source: "WEBRip",
    codec: "HEVC",
    hdr: null,
    audio: "AAC 2.0",
    gb: 0.48,
    seeders: 71,
    leechers: 6,
    ageDays: 5,
    from: ["eztv"],
    torrent: true,
    upgrade: "worse",
  },
];

function fileSafe(text: string): string {
  return text.replace(/[^A-Za-z0-9]+/g, ".").replace(/^\.|\.$/g, "");
}

function hashOf(seed: string, index: number): string {
  let value = 2166136261;
  for (const char of `${seed}${index}`) value = Math.imul(value ^ char.charCodeAt(0), 16777619);
  const part = (value >>> 0).toString(16).padStart(8, "0");
  return part.repeat(5);
}

export interface SyntheticTarget {
  kind: TitleKind;
  tmdbId: number;
  season?: number;
  episode?: number;
}

const two = (n: number) => String(n).padStart(2, "0");

/** The rows of a target and the name stem of each. */
function rowsOf(entry: CatalogTitle, target: SyntheticTarget): { row: Row; stem: string }[] {
  const base = fileSafe(entry.title);
  if (entry.kind === "movie") {
    return MOVIE_ROWS.map((row) => ({
      row,
      stem:
        row.other === undefined
          ? `${base}.${entry.year}`
          : `${base}.${row.other}.${entry.year + 3}`,
    }));
  }
  const season = target.season ?? 1;
  const packs = PACK_ROWS.map((row) => ({
    row,
    stem:
      row.pack === "seasons"
        ? `${base}.S01-S${two(Math.max(season, 2))}`
        : `${base}.S${two(season)}`,
  }));
  if (target.season === undefined) {
    return packs.map(({ row, stem }) => ({
      row: { ...row, pack: "complete" as const },
      stem: stem.replace(/\.S\d+(-S\d+)?$/, ".Complete.Series"),
    }));
  }
  if (target.episode === undefined) {
    return [
      ...packs,
      ...EPISODE_ROWS.slice(0, 2).map((row) => ({ row, stem: `${base}.S${two(season)}E01` })),
    ];
  }
  const episode = `${base}.S${two(season)}E${two(target.episode)}`;
  return [...packs, ...EPISODE_ROWS.map((row) => ({ row, stem: episode }))];
}

/** The releases of a target, as the sources return them. */
export function syntheticReleases(
  target: SyntheticTarget,
  now: number = Date.UTC(2026, 9, 26),
): TorrentReleaseDto[] {
  const entry = catalogTitle(target.kind, target.tmdbId);
  if (entry === undefined || entry.behaviour === "empty") return [];
  const seed = `${entry.kind}${entry.tmdbId}${target.season ?? ""}${target.episode ?? ""}`;
  return rowsOf(entry, target).map(({ row, stem }, index) => {
    const hash = hashOf(seed, index);
    const name = row.tail.startsWith("[")
      ? `${entry.title} (${entry.year}) ${row.tail.replace(/\]\.\[/g, "] [")}`
      : `${stem}.${row.tail}`;
    const sources = row.from.filter((id) => {
      const source = SOURCES.find((candidate) => candidate.id === id);
      return source !== undefined && (entry.kind === "movie" ? source.movies : source.tv);
    });
    const from = sources.length === 0 ? ["torrentio"] : sources;
    const pages = from.flatMap((source) => {
      const url = pageOf(source, entry, index + 1);
      return url === null ? [] : [{ source, detailsUrl: url }];
    });
    return {
      id: hash,
      absorbs: [],
      name,
      size: Math.round(row.gb * GB),
      seeders: row.seeders,
      leechers: row.leechers,
      published: now - row.ageDays * 86_400_000,
      sources: from,
      pages,
      magnet: `magnet:?xt=urn:btih:${hash}&dn=${encodeURIComponent(name)}`,
      torrentUrl: row.torrent === true ? `https://example.invalid/dl/${hash}.torrent` : null,
      infoHash: hash,
      resolution: row.resolution,
      source: row.source,
      codec: row.codec,
      hdr: row.hdr,
      audio: row.audio,
      pack: row.pack ?? null,
      flags: (row.flags ?? []).map((flag) => ({ ...flag })),
      upgrade: entry.inLibrary === null ? null : (row.upgrade ?? null),
      matching: row.other === undefined,
    } satisfies TorrentReleaseDto;
  });
}

// ---------------------------------------------------------------- landing

function landingTitle(entry: CatalogTitle): TorrentLandingTitleDto {
  return {
    itemId: `${entry.kind}-${entry.tmdbId}`,
    kind: entry.kind,
    tmdbId: entry.tmdbId,
    title: entry.title,
    year: entry.year,
    poster: null,
    resolution: entry.kind === "movie" ? (entry.copy?.resolution ?? null) : null,
    added: entry.addedDaysAgo === undefined ? null : Date.now() - entry.addedDaysAgo * 86_400_000,
  };
}

const RANK: Readonly<Record<string, number>> = { "480p": 0, "720p": 1, "1080p": 2 };

/** The landing strips of the synthetic library. */
export function syntheticLanding(): {
  upgrades: TorrentLandingTitleDto[];
  upgradeTotal: number;
  recent: TorrentLandingTitleDto[];
} {
  const owned = CATALOG.filter((entry) => entry.inLibrary !== null);
  const upgrades = owned
    .filter(
      (entry) =>
        entry.kind === "movie" &&
        entry.copy !== undefined &&
        RANK[entry.copy.resolution] !== undefined,
    )
    .sort((a, b) => (RANK[a.copy?.resolution ?? ""] ?? 3) - (RANK[b.copy?.resolution ?? ""] ?? 3))
    .map(landingTitle);
  const recent = owned
    .filter((entry) => entry.addedDaysAgo !== undefined)
    .sort((a, b) => (a.addedDaysAgo ?? 0) - (b.addedDaysAgo ?? 0))
    .slice(0, 8)
    .map(landingTitle);
  return { upgrades: upgrades.slice(0, 8), upgradeTotal: upgrades.length, recent };
}
