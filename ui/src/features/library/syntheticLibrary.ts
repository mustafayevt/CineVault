import { syntheticLifecycle, syntheticLifecycleCounts } from "../lifecycle/syntheticLifecycle";
import type { SyntheticLifecycleInput } from "../lifecycle/syntheticLifecycle";
import { currentPreferredSource } from "../ratings/preference";
import { DEFAULT_PREFERRED_SOURCE, pickRating, toNativeScale } from "../ratings/ratingSources";
import type { SourceRating } from "../ratings/ratingSources";
import { libraryFolderOf } from "./titleFacts";
import type {
  FacetValue,
  FilterDef,
  LibraryBackend,
  LibraryCard,
  LibraryChip,
  LibraryFacet,
  LibraryKind,
  LibraryQuery,
  LibraryRow,
  LibrarySortField,
} from "./libraryBackend";

/**
 * A deterministic fixture library for a plain browser (no desktop backend), tests and the
 * scrolling trace. Items are generated from their index, so 20,000 of them cost nothing until a
 * window is read.
 */

const WORDS = [
  "Midnight",
  "Harbour",
  "Silent",
  "Crimson",
  "Paper",
  "Glass",
  "Northern",
  "Last",
  "Electric",
  "Hollow",
  "Golden",
  "Winter",
  "Distant",
  "Iron",
  "Velvet",
  "Burning",
  "Quiet",
  "Broken",
  "Neon",
  "Salt",
  "Orbit",
  "Garden",
  "Machine",
  "Letters",
  "Tide",
  "Empire",
  "Lantern",
  "Border",
] as const;

const RESOLUTIONS = ["2160p", "1080p", "1080p", "720p", "2160p", "1080p"] as const;
const HDR = [null, null, "DV", "HDR10", null, null, "HDR10+", null] as const;

// ThumbHashes of 2:3 gradients (produced by the Rust encoder) so tiles show their preview.
const THUMBHASHES = [
  "d1690a2d0888887f7787778777788887887f87f887",
  "8ce705050088887f78878787877887877680770778",
  "4db6091d0688887f77877788777888877780770778",
  "8c78060d0878887f7787878887888887787f87f887",
  "0db8090d0478887f7778878887888887888f780788",
  "4d190a1d0278887f7788878887788887887f88f777",
  "ce260a150278787f7787778877788787878088f777",
  "d7180e0d0278887f7788788777888787787f88f777",
] as const;

const CERTIFICATIONS = ["PG", "PG-13", "R", "TV-MA", "TV-14"] as const;
const CODECS = ["HEVC", "H.264", "AV1", "HEVC"] as const;
const SOURCES = ["BluRay", "WEB-DL", "UHD BluRay", "WEB-DL"] as const;
const STATUSES = ["continuing", "ended", "canceled", "ended"] as const;
const AUDIO = ["TrueHD Atmos", "DTS-HD MA", "AC3", "E-AC-3", "AAC"] as const;
const HEIGHT: Readonly<Record<string, number>> = { "2160p": 2160, "1080p": 1080, "720p": 720 };
const DAY_MS = 86_400_000;
const BASE_ADDED_MS = Date.UTC(2023, 0, 1);

const collator = new Intl.Collator("en", { sensitivity: "base" });

/** A small deterministic integer hash, so the same index always gives the same item. */
function mix(n: number): number {
  let h = Math.imul(n ^ 0x9e3779b9, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  return (h ^ (h >>> 16)) >>> 0;
}

function titleOf(index: number): string {
  const a = WORDS[mix(index) % WORDS.length] ?? "Untitled";
  const b = WORDS[mix(index + 7919) % WORDS.length] ?? "Film";
  // Every word pair repeats after 784 titles, so later ones get a number to stay distinct.
  const round = Math.floor(index / 784);
  return round > 0 ? `${a} ${b} ${round + 1}` : `${a} ${b}`;
}

/** The facts a fixture title's lifecycle is generated from. */
function lifecycleInput(card: LibraryCard, index: number, show: boolean): SyntheticLifecycleInput {
  return {
    index,
    title: card.title,
    year: card.year,
    unmatched: card.unmatched,
    needsReview: card.needs_review,
    show,
  };
}

/** The fixture item at `index` (0-based, in insertion order). */
export function syntheticCard(index: number): LibraryCard {
  const h = mix(index + 1);
  const title = titleOf(index);
  const unmatched = h % 29 === 0;
  const hasPoster = !unmatched && h % 11 !== 0;
  const hash = THUMBHASHES[h % THUMBHASHES.length] ?? THUMBHASHES[0];
  const card: LibraryCard = {
    id: String(index + 1),
    title,
    original_title: !unmatched && h % 6 === 0 ? `${title} (Original)` : "",
    year: 1960 + (h % 66),
    is_new: h % 17 === 0,
    needs_review: h % 23 === 0,
    lifecycle: null,
    unmatched,
    watched: h % 5 === 0,
    file_name: `${title.replaceAll(" ", ".")}.${1960 + (h % 66)}.2160p.mkv`,
    resolution: unmatched ? null : (RESOLUTIONS[h % RESOLUTIONS.length] ?? null),
    hdr: unmatched ? null : (HDR[h % HDR.length] ?? null),
    poster: hasPoster
      ? {
          key: (h.toString(16) + mix(h).toString(16)).padEnd(32, "0").slice(0, 32),
          thumbhash: hash,
          ambient: null,
        }
      : null,
  };
  const lifecycle = syntheticLifecycle(lifecycleInput(card, index, false));
  // The REVIEW marker follows the lifecycle, as the backend's does.
  return { ...card, lifecycle, needs_review: lifecycle.in_review };
}

/** Per-source ratings (0 to 10) of a fixture title; not every title has every source. */
interface SourceFields {
  imdb_rating: number | null;
  tmdb_rating: number | null;
  trakt_rating: number | null;
  rotten_tomatoes_rating: number | null;
  rotten_tomatoes_audience_rating: number | null;
  metacritic_rating: number | null;
  letterboxd_rating: number | null;
  myanimelist_rating: number | null;
  rogerebert_rating: number | null;
}

const clampTen = (value: number): number => Math.round(Math.min(10, Math.max(1, value)) * 10) / 10;

function sourceFields(rating: number | null, h: number, g: number, show: boolean): SourceFields {
  const has = (condition: boolean, value: number): number | null =>
    rating !== null && condition ? clampTen(value) : null;
  return {
    // A quarter of the titles have no IMDb rating, so a preferred IMDb shows stand-ins.
    imdb_rating: has(h % 4 !== 0, (rating ?? 0) + 0.2),
    tmdb_rating: has(true, rating ?? 0),
    trakt_rating: has(h % 3 === 0, (rating ?? 0) - 0.3),
    rotten_tomatoes_rating: has(true, (rating ?? 0) + 0.7),
    rotten_tomatoes_audience_rating: has(h % 2 === 0, (rating ?? 0) + 0.4),
    metacritic_rating: has(h % 3 !== 0, (rating ?? 0) - 0.9),
    letterboxd_rating: has(!show && h % 3 !== 1, (rating ?? 0) - 0.2),
    myanimelist_rating: has(g % 13 === 0, (rating ?? 0) + 0.1),
    rogerebert_rating: has(!show && h % 11 === 0, (rating ?? 0) + 0.3),
  };
}

/** The source ratings of a row, as the preferred-rating rules read them (0 to 10). */
function sourceRatingsOf(row: LibraryRow): SourceRating[] {
  const entries: [string, number | null][] = [
    ["imdb", row.imdb_rating],
    ["tmdb", row.tmdb_rating],
    ["trakt", row.trakt_rating],
    ["tomatometerallcritics", row.rotten_tomatoes_rating],
    ["tomatometerallaudience", row.rotten_tomatoes_audience_rating],
    ["metacritic", row.metacritic_rating],
    ["letterboxd", row.letterboxd_rating],
    ["myanimelist", row.myanimelist_rating],
    ["rogerebert", row.rogerebert_rating],
  ];
  return entries.flatMap(([source, value]) => (value === null ? [] : [{ source, value, max: 10 }]));
}

/**
 * The rating a row shows for a preferred source: the preferred source's, or the next available
 * one, marked as a stand-in. The same rule as the database's `table_facts_for`.
 */
function withPreferredRating(row: LibraryRow, preferred: string): LibraryRow {
  const picked = pickRating(sourceRatingsOf(row), preferred);
  if (picked === null) {
    return { ...row, rating: null, rating_source: null, rating_is_fallback: false, votes: 0 };
  }
  return {
    ...row,
    rating: picked.rating.value,
    rating_source: picked.rating.source,
    rating_is_fallback: picked.fallback,
    votes: 50 + (mix(Number(row.card.id)) % 250_000),
  };
}

/** The fixture row at `index`: the card plus every column of the table, deterministic. */
export function syntheticRow(index: number, kind: LibraryKind = "movies"): LibraryRow {
  const base = syntheticCard(index);
  const card =
    kind === "shows"
      ? { ...base, lifecycle: syntheticLifecycle(lifecycleInput(base, index, true)) }
      : base;
  const h = mix(index + 1);
  const g = mix(h);
  const matched = !card.unmatched;
  const height = card.resolution === null ? 0 : (HEIGHT[card.resolution] ?? 0);
  const rating = matched && h % 9 !== 0 ? 3 + (g % 61) / 10 : null;
  const hasVideo = matched || h % 2 === 0;
  const fileSize = hasVideo ? (1 + (g % 70)) * 1_000_000_000 + (h % 1000) * 1_000_000 : 0;
  const show = kind === "shows";
  const month = String(1 + (h % 12)).padStart(2, "0");
  const day = String(1 + (g % 28)).padStart(2, "0");
  const row: LibraryRow = {
    card,
    original_title: card.original_title,
    sort_title: card.title.replace(/^The /, ""),
    plot_excerpt: matched
      ? `A ${card.title.toLowerCase()} story told over ${40 + (h % 120)} minutes, with a long middle and a short end.`
      : "",
    release_date: matched ? `${card.year}-${month}-${day}` : null,
    runtime_minutes: matched ? 70 + (h % 110) : 0,
    path: `/media/${kind}/${card.file_name.replace(/\.mkv$/, "")}`,
    source_label: h % 3 === 0 ? "Archive" : "Movies",
    filename: hasVideo ? card.file_name : "",
    movie_set:
      kind === "movies" && matched && h % 8 === 0 ? `${titleOf(index % 40)} Collection` : null,
    certification: matched ? (CERTIFICATIONS[h % CERTIFICATIONS.length] ?? "") : "",
    edition: matched && h % 13 === 0 ? "Director's Cut" : "",
    media_source: matched ? (SOURCES[h % SOURCES.length] ?? "") : "",
    note: h % 31 === 0 ? "Check the subtitles" : "",
    locked: false,
    video_in_3d: h % 41 === 0,
    date_added_ms: BASE_ADDED_MS + index * (DAY_MS / 6),
    top250: matched && h % 97 === 0 ? 1 + (g % 250) : 0,
    // The preferred rating is chosen below, so every row starts without one.
    rating: null,
    rating_source: null,
    rating_is_fallback: false,
    votes: 0,
    ...sourceFields(rating, h, g, show),
    user_rating: h % 7 === 0 ? 5 + (g % 6) : null,
    video_codec: hasVideo ? (CODECS[h % CODECS.length] ?? "") : "",
    container: hasVideo ? "Matroska" : "",
    width: Math.round((height * 16) / 9),
    height,
    aspect_ratio: hasVideo ? (h % 3 === 0 ? 2.39 : 1.78) : 0,
    aspect_ratio2: hasVideo && h % 3 === 0 ? 1.85 : 0,
    video_bitrate_kbps: hasVideo ? 4000 + (g % 60) * 500 : 0,
    hdr_formats: card.hdr === null ? [] : [card.hdr === "DV" ? "Dolby Vision" : card.hdr],
    file_size: fileSize,
    total_file_size: fileSize + (hasVideo ? 90_000_000 : 0),
    file_modified_ms: hasVideo ? BASE_ADDED_MS - (h % 400) * DAY_MS : null,
    audio_codec: hasVideo ? (AUDIO[h % AUDIO.length] ?? "") : "",
    audio_channels: hasVideo ? (h % 5 === 0 ? 8 : h % 3 === 0 ? 2 : 6) : 0,
    audio_streams: hasVideo ? 1 + (h % 3) : 0,
    subtitle_streams: hasVideo ? h % 4 : 0,
    trailers: matched ? h % 3 : 0,
    has_poster: card.poster !== null,
    has_fanart: matched && h % 4 !== 0,
    has_logo: matched && h % 3 !== 0,
    has_nfo: matched,
    subtitle_files: hasVideo && h % 7 === 0 ? 1 : 0,
    metadata_complete: matched && h % 5 !== 0,
    seasons: show ? 1 + (h % 6) : 0,
    episodes: show ? 8 + (g % 90) : 0,
    missing_episodes: show ? h % 5 : 0,
    movies_in_set: kind === "collections" ? 2 + (h % 5) : 0,
    missing_movies: kind === "collections" ? h % 3 : 0,
    aired_status: show ? (STATUSES[h % STATUSES.length] ?? "unknown") : "unknown",
  };
  return withPreferredRating(row, DEFAULT_PREFERRED_SOURCE);
}

const inRange = (value: number | null, min: number | null, max: number | null): boolean =>
  value !== null && (min === null || value >= min) && (max === null || value <= max);

const sameText = (a: string, b: string): boolean => a.toLowerCase() === b.toLowerCase();

/**
 * Whether a fixture row satisfies one chip, like the database does. Fields the fixtures carry no
 * data for (people, genres, tags, languages) restrict nothing.
 */
function matchesChip(row: LibraryRow, chip: LibraryChip): boolean {
  const any = (values: readonly string[], actual: string): boolean =>
    values.some((value) => sameText(value, actual));
  const rated = (...ratings: (number | null)[]): boolean =>
    ratings.some((rating) => inRange(rating, chip.min, chip.max));
  let matches: boolean;
  switch (chip.field) {
    case "watched":
      matches = row.card.watched;
      break;
    case "new":
      matches = row.card.is_new;
      break;
    case "review":
      matches = row.card.needs_review;
      break;
    case "not_matched":
    case "rename_pending":
    case "missing": {
      // The lifecycle flags, as the backend's lifecycle filter reads them.
      const kind = chip.field.replace("_", "-");
      matches = row.card.lifecycle?.flags.some((entry) => entry.kind === kind) ?? false;
      break;
    }
    case "locked":
      matches = row.locked;
      break;
    case "year":
      matches = row.card.year > 0 && inRange(row.card.year, chip.min, chip.max);
      break;
    case "runtime":
      matches = row.runtime_minutes > 0 && inRange(row.runtime_minutes, chip.min, chip.max);
      break;
    case "top250":
      matches = row.top250 > 0 && inRange(row.top250, chip.min, chip.max);
      break;
    case "rating":
      // The preferred rating, or the next available source, like the database.
      matches = rated(row.rating);
      break;
    case "imdb_rating":
      matches = rated(row.imdb_rating);
      break;
    case "tmdb_rating":
      matches = rated(row.tmdb_rating);
      break;
    case "rotten_tomatoes_rating":
      matches = rated(row.rotten_tomatoes_rating);
      break;
    case "metacritic_rating":
      matches = rated(row.metacritic_rating);
      break;
    case "user_rating":
      matches = rated(row.user_rating);
      break;
    case "resolution":
      matches = row.card.resolution !== null && any(chip.values, row.card.resolution);
      break;
    case "hdr":
      matches = row.hdr_formats.some((format) => any(chip.values, format));
      break;
    case "codec":
      matches = row.video_codec !== "" && any(chip.values, row.video_codec);
      break;
    case "certification":
      matches = row.certification !== "" && any(chip.values, row.certification);
      break;
    case "edition":
      matches = row.edition !== "" && any(chip.values, row.edition);
      break;
    case "source":
      matches = row.media_source !== "" && any(chip.values, row.media_source);
      break;
    case "library_source":
      matches = any(chip.values, row.source_label);
      break;
    case "stage":
      matches = row.card.lifecycle !== null && any(chip.values, row.card.lifecycle.stage);
      break;
    case "title":
    case "original_title":
    case "sort_title": {
      const text = (
        chip.field === "title"
          ? row.card.title
          : chip.field === "original_title"
            ? row.original_title
            : row.sort_title
      ).toLowerCase();
      matches = chip.values.some((value) => text.includes(value.toLowerCase()));
      break;
    }
    default:
      return true;
  }
  // A yes/no chip is negated by its flag in the database; for a fixture, inverting is the same.
  return chip.negated ? !matches : matches;
}

// ---- the filter catalogue ----------------------------------------------------------------------------

const def = (
  id: string,
  label: string,
  group: FilterDef["group"],
  kind: FilterDef["kind"],
  unit: string | null = null,
): FilterDef => ({ id, label, group, kind, unit });

/** The filters the fixtures carry data for; the desktop app offers the full catalogue. */
const SHARED_FILTERS: readonly FilterDef[] = [
  def("new", "New", "details", "flag"),
  def("locked", "Locked", "details", "flag"),
  def("date_added", "Date added", "details", "date"),
  def("year", "Year", "details", "number"),
  def("certification", "Certification", "details", "multi"),
  def("edition", "Edition", "details", "multi"),
  def("media_source", "Release", "details", "multi"),
  def("runtime", "Runtime", "details", "number", "min"),
  def("video_format", "Resolution", "video", "multi"),
  def("video_codec", "Video codec", "video", "multi"),
  def("video_bitrate", "Video bitrate", "video", "number", "kbps"),
  def("video_container", "Container", "video", "multi"),
  def("hdr_format", "HDR format", "video", "multi"),
  def("audio_codec", "Audio codec", "audio", "multi"),
  def("audio_stream_count", "Audio streams", "audio", "number"),
  def("subtitle_count", "Subtitle count", "subtitles", "number"),
  def("video_filesize", "Video file size", "files", "number", "MiB"),
  def("filename", "Any file name", "files", "text"),
];

/** The rating filters (`cv_db` facet ids): per source, in the units the source reports. */
const RATING_FILTERS: readonly FilterDef[] = [
  def("top250_rank", "IMDb Top 250 rank", "ratings", "number"),
  def("rating_imdb", "IMDb rating", "ratings", "number"),
  def("rating_tmdb", "TMDB rating", "ratings", "number"),
  def("rating_trakt", "Trakt rating", "ratings", "number"),
  def("rating_tvdb", "TVDB rating", "ratings", "number"),
  def("rating_metacritic", "Metacritic score", "ratings", "number", "/100"),
  def("rating_rt_critics", "Rotten Tomatoes critics", "ratings", "number", "%"),
  def("rating_rt_audience", "Rotten Tomatoes audience", "ratings", "number", "%"),
  def("rating_letterboxd", "Letterboxd rating", "ratings", "number", "/5"),
  def("rating_my_anime_list", "MyAnimeList rating", "ratings", "number"),
  def("rating_roger_ebert", "Roger Ebert rating", "ratings", "number", "/4"),
  def("rating_user", "My rating", "ratings", "number"),
];

/** The synthetic catalogue of a library: the shared filters, a show's status and the ratings. */
function syntheticCatalogue(kind: LibraryKind): readonly FilterDef[] {
  if (kind === "collections") return [];
  return kind === "shows"
    ? [...SHARED_FILTERS, def("status", "Status", "details", "multi"), ...RATING_FILTERS]
    : [...SHARED_FILTERS, ...RATING_FILTERS];
}

type Owned =
  | { t: "flag"; on: boolean }
  | { t: "list"; values: readonly string[] }
  | { t: "num"; value: number | null }
  | { t: "date"; value: string | null };

const list = (...values: (string | null | undefined)[]): Owned => ({
  t: "list",
  values: values.filter(
    (value): value is string => value !== null && value !== undefined && value !== "",
  ),
});
const num = (value: number): Owned => ({ t: "num", value: value > 0 ? value : null });

/** A rating (0 to 10 on the row) in the units its source reports, like the stored value. */
const nativeRating = (source: string, valueOnTen: number | null): Owned => ({
  t: "num",
  value: valueOnTen === null ? null : Math.round(toNativeScale(source, valueOnTen) * 1000) / 1000,
});

/** What a fixture row has for a filter, or `undefined` for a filter the fixtures know nothing of. */
function ownedOf(row: LibraryRow, id: string): Owned | undefined {
  switch (id) {
    case "watched":
      return { t: "flag", on: row.card.watched };
    case "new":
      return { t: "flag", on: row.card.is_new };
    case "locked":
      return { t: "flag", on: row.locked };
    case "date_added":
      return {
        t: "date",
        value:
          row.date_added_ms === null
            ? null
            : new Date(row.date_added_ms).toISOString().slice(0, 10),
      };
    case "year":
      return num(row.card.year);
    case "runtime":
      return num(row.runtime_minutes);
    case "video_bitrate":
      return num(row.video_bitrate_kbps);
    case "audio_stream_count":
      return num(row.audio_streams);
    case "subtitle_count":
      return num(row.subtitle_streams);
    case "video_filesize":
      return num(Math.round((row.file_size ?? 0) / 1_048_576));
    case "certification":
      return list(row.certification);
    case "edition":
      return list(row.edition);
    case "media_source":
      return list(row.media_source);
    case "video_format":
      return list(row.card.resolution);
    case "video_codec":
      return list(row.video_codec);
    case "video_container":
      return list(row.container);
    case "hdr_format":
      // SDR is a value of its own: known media info and no HDR format, as in the library.
      return row.hdr_formats.length === 0 && row.height > 0
        ? list("SDR")
        : list(...row.hdr_formats);
    case "audio_codec":
      return list(row.audio_codec);
    case "status":
      return list(row.aired_status);
    case "top250_rank":
      return num(row.top250);
    case "rating_imdb":
      return nativeRating("imdb", row.imdb_rating);
    case "rating_tmdb":
      return nativeRating("tmdb", row.tmdb_rating);
    case "rating_trakt":
      return nativeRating("trakt", row.trakt_rating);
    case "rating_tvdb":
      return { t: "num", value: null };
    case "rating_metacritic":
      return nativeRating("metacritic", row.metacritic_rating);
    case "rating_rt_critics":
      return nativeRating("tomatometerallcritics", row.rotten_tomatoes_rating);
    case "rating_rt_audience":
      return nativeRating("tomatometerallaudience", row.rotten_tomatoes_audience_rating);
    case "rating_letterboxd":
      return nativeRating("letterboxd", row.letterboxd_rating);
    case "rating_my_anime_list":
      return nativeRating("myanimelist", row.myanimelist_rating);
    case "rating_roger_ebert":
      return nativeRating("rogerebert", row.rogerebert_rating);
    case "rating_user":
      return { t: "num", value: row.user_rating };
    case "filename":
      return list(row.filename);
    default:
      return undefined;
  }
}

function compareNumber(
  op: LibraryFacet["op"],
  value: number,
  a: number,
  b: number | null,
): boolean {
  switch (op) {
    case "lt":
      return value < a;
    case "le":
      return value <= a;
    case "eq":
      return value === a;
    case "ge":
      return value >= a;
    case "gt":
      return value > a;
    case "between":
      return b !== null && value >= Math.min(a, b) && value <= Math.max(a, b);
  }
}

function compareDate(
  op: LibraryFacet["op"],
  value: string,
  a: string,
  b: string | undefined,
): boolean {
  const cut = (date: string, like: string) => date.slice(0, like.length);
  switch (op) {
    case "lt":
      return cut(value, a) < a;
    case "le":
      return cut(value, a) <= a;
    case "eq":
      return cut(value, a) === a;
    case "ge":
      return cut(value, a) >= a;
    case "gt":
      return cut(value, a) > a;
    case "between":
      return b !== undefined && cut(value, a) >= a && cut(value, b) <= b;
  }
}

/** Whether a fixture row satisfies one catalogue chip, with the rules of the database. */
function matchesFacet(row: LibraryRow, facet: LibraryFacet): boolean {
  const owned = ownedOf(row, facet.id);
  if (owned === undefined) return true;
  let matches: boolean;
  switch (owned.t) {
    case "flag":
      matches = owned.on;
      break;
    case "list": {
      if (facet.values.length === 0) return true;
      const has = (wanted: string) => owned.values.some((value) => sameText(value, wanted));
      matches = facet.mode === "all" ? facet.values.every(has) : facet.values.some(has);
      break;
    }
    case "num":
      if (facet.number === null || (facet.op === "between" && facet.number2 === null)) return true;
      matches =
        owned.value !== null && compareNumber(facet.op, owned.value, facet.number, facet.number2);
      break;
    case "date": {
      const first = facet.values[0];
      if (first === undefined) return true;
      matches = owned.value !== null && compareDate(facet.op, owned.value, first, facet.values[1]);
      break;
    }
  }
  return facet.negated ? !matches : matches;
}

/**
 * Folds text the way the library's search does: lower case, no diacritics or apostrophes, `&` as
 * "and", other punctuation as a space (`Spider-Man` is `spider man`).
 */
export function foldForSearch(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/['’‘`]/g, "")
    .replace(/&/g, " and ")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

/**
 * A title matches when every word of the search starts a word of it (or of its original title),
 * with spaces and punctuation ignored: "spiderman" finds "Spider-Man" and "amelie" finds "Amélie".
 */
function matchesText(row: LibraryRow, words: readonly string[]): boolean {
  const titles = [row.card.title, row.original_title].map(foldForSearch);
  const spaced = titles.join(" | ");
  const compact = titles.map((title) => title.replaceAll(" ", ""));
  const phrase = words.join("");
  return (
    words.every((word) => spaced.split(/[ |]+/).some((w) => w.startsWith(word))) ||
    compact.some((title) => title.startsWith(phrase))
  );
}

type SortValue = number | string | null;

/** What a sort field reads from a fixture row; fields it does not know compare as equal. */
const SORT_VALUE: Readonly<Partial<Record<LibrarySortField, (row: LibraryRow) => SortValue>>> = {
  title: (r) => r.card.title,
  sort_title: (r) => r.sort_title || r.card.title,
  original_title: (r) => r.original_title || r.card.title,
  year: (r) => (r.card.year > 0 ? r.card.year : null),
  date_added: (r) => r.date_added_ms,
  release_date: (r) => r.release_date,
  runtime: (r) => (r.runtime_minutes > 0 ? r.runtime_minutes : null),
  path: (r) => r.path,
  folder: (r) => libraryFolderOf(r.path),
  quality: (r) => qualityRank(r),
  source: (r) => r.source_label,
  stage: (r) => ({ new: 0, scraped: 1, renamed: 2 })[r.card.lifecycle?.stage ?? "new"],
  filename: (r) => r.filename || null,
  top250: (r) => (r.top250 > 0 ? r.top250 : null),
  certification: (r) => r.certification,
  edition: (r) => r.edition,
  media_source: (r) => r.media_source,
  watched: (r) => Number(r.card.watched),
  is_new: (r) => Number(r.card.is_new),
  movie_set: (r) => r.movie_set,
  rating: (r) => r.rating,
  votes: (r) => (r.rating === null ? null : r.votes),
  user_rating: (r) => r.user_rating,
  imdb_rating: (r) => r.imdb_rating,
  tmdb_rating: (r) => r.tmdb_rating,
  trakt_rating: (r) => r.trakt_rating,
  rotten_tomatoes_rating: (r) => r.rotten_tomatoes_rating,
  rotten_tomatoes_audience_rating: (r) => r.rotten_tomatoes_audience_rating,
  metacritic_rating: (r) => r.metacritic_rating,
  letterboxd_rating: (r) => r.letterboxd_rating,
  my_anime_list_rating: (r) => r.myanimelist_rating,
  roger_ebert_rating: (r) => r.rogerebert_rating,
  tvdb_rating: () => null,
  video_height: (r) => (r.height > 0 ? r.height : null),
  aspect_ratio: (r) => (r.aspect_ratio ?? 0) || null,
  aspect_ratio2: (r) => (r.aspect_ratio2 ?? 0) || null,
  video_codec: (r) => r.video_codec || null,
  video_bitrate: (r) => r.video_bitrate_kbps || null,
  hdr: (r) => Number(r.hdr_formats.length > 0),
  file_size: (r) => r.file_size || null,
  total_file_size: (r) => (r.total_file_size ?? 0) || null,
  file_modified: (r) => r.file_modified_ms,
  audio_streams: (r) => r.audio_streams,
  subtitle_streams: (r) => r.subtitle_streams,
  trailers: (r) => r.trailers,
  images: (r) => Number(r.has_poster) + Number(r.has_fanart),
  metadata: (r) => Number(r.metadata_complete),
  seasons: (r) => r.seasons,
  episodes: (r) => r.episodes,
  missing_episodes: (r) => r.missing_episodes,
  movies_in_set: (r) => r.movies_in_set,
  missing_movies: (r) => r.missing_movies,
  aired_status: (r) => r.aired_status,
};

/** The backend's rank: resolution band, then HDR on top; no known height sorts last. */
function qualityRank(r: LibraryRow): number | null {
  const h = r.height;
  if (h <= 0) return null;
  const band = h >= 1500 ? 8 : h >= 760 ? 6 : h >= 600 ? 4 : 2;
  return band + (r.hdr_formats.length > 0 ? 1 : 0);
}

/** Like the database: text ignores case. Nothing (null) is handled by `compare`. */
function compareValues(a: number | string, b: number | string): number {
  if (typeof a === "number" && typeof b === "number") return a - b;
  return collator.compare(String(a), String(b));
}

function compare(a: LibraryRow, b: LibraryRow, query: LibraryQuery): number {
  for (const key of query.sort) {
    const read = SORT_VALUE[key.field];
    if (read === undefined) continue;
    const left = read(a);
    const right = read(b);
    // Like the database: nothing sorts after every value, whichever way the column is sorted.
    if (left === null || right === null) {
      if (left === right) continue;
      return left === null ? 1 : -1;
    }
    const order = compareValues(left, right);
    if (order !== 0) return key.descending ? -order : order;
  }
  // The title, then the insertion order, so the order is always total.
  return collator.compare(a.card.title, b.card.title) || Number(a.card.id) - Number(b.card.id);
}

function totalFor(kind: LibraryKind, count: number): number {
  if (kind === "movies") return count;
  return Math.ceil(count / (kind === "shows" ? 8 : 12));
}

/** The pipeline counts of the first `count` fixture movies, or of the first `count` shows. */
export function syntheticPipelineCounts(count: number, show: boolean) {
  return syntheticLifecycleCounts(count, (index) =>
    lifecycleInput(syntheticCard(index), index, show),
  );
}

function syntheticNeedsReview(count: number, show: boolean): number {
  return syntheticPipelineCounts(count, show).needs_review;
}

/** An in-memory backend over `count` generated movies (and proportionally fewer shows and sets). */
export function createSyntheticBackend(count: number): LibraryBackend {
  const watched = new Map<string, boolean>();
  // Bumped when a watched flag changes, so cached orders (which filters read) are rebuilt.
  let watchedVersion = 0;
  const stateOf = (id: string, flag: boolean) => ({
    id,
    watched: flag,
    play_count: flag ? 1 : 0,
    last_watched_ms: null,
  });
  const ordered = new Map<string, LibraryRow[]>();
  const rowsFor = (query: LibraryQuery): LibraryRow[] => {
    const preferred = query.rating_source ?? currentPreferredSource();
    const cacheKey = JSON.stringify([
      query.kind,
      query.sort,
      query.search,
      query.filters,
      query.facets ?? [],
      preferred,
      watchedVersion,
    ]);
    let rows = ordered.get(cacheKey);
    if (rows === undefined) {
      const words = foldForSearch(query.search).split(" ").filter(Boolean);
      rows = Array.from({ length: totalFor(query.kind, count) }, (_, i) =>
        withWatched(withPreferredRating(syntheticRow(i, query.kind), preferred)),
      ).filter(
        (row) =>
          matchesText(row, words) &&
          query.filters.every((chip) => matchesChip(row, chip)) &&
          (query.facets ?? []).every((facet) => matchesFacet(row, facet)),
      );
      rows.sort((a, b) => compare(a, b, query));
      ordered.set(cacheKey, rows);
    }
    return rows;
  };
  const withWatched = (row: LibraryRow): LibraryRow => {
    const flag = watched.get(row.card.id);
    return flag === undefined ? row : { ...row, card: { ...row.card, watched: flag } };
  };
  return {
    window: (query, offset, limit) => {
      const rows = rowsFor(query);
      const items = rows.slice(offset, offset + limit).map((row) => withWatched(row).card);
      return Promise.resolve({ total: rows.length, offset, items });
    },
    rows: (query, offset, limit) => {
      const rows = rowsFor(query);
      const items = rows.slice(offset, offset + limit).map(withWatched);
      return Promise.resolve({ total: rows.length, offset, items });
    },
    counts: () =>
      Promise.resolve({
        movies: count,
        tv_shows: totalFor("shows", count),
        collections: totalFor("collections", count),
        // The lifecycle's figure: uncertain matches plus titles that could not be matched.
        needs_review:
          syntheticNeedsReview(count, false) + syntheticNeedsReview(totalFor("shows", count), true),
        new_items: Math.floor(count / 17) + Math.floor(count / 60),
        new_movies: Math.floor(count / 17),
        new_shows: Math.floor(count / 60),
        unmatched: Math.ceil(count / 29),
      }),
    sizeTotal: (kind) => {
      // Every fixture file once; the offline drive of the sidebar holds a fixed 2 TB more.
      let counted = 0;
      for (let i = 0; i < totalFor(kind, count); i += 1) {
        counted += syntheticRow(i, kind).total_file_size ?? 0;
      }
      return Promise.resolve({ counted_bytes: counted, offline_bytes: 2_000_000_000_000 });
    },
    sourceGroups: (query) => {
      const groups = new Map<string, number>();
      for (const row of rowsFor(query)) {
        groups.set(row.source_label, (groups.get(row.source_label) ?? 0) + 1);
      }
      return Promise.resolve(
        [...groups.entries()]
          .sort(([a], [b]) => collator.compare(a, b))
          .map(([label, total]) => ({ label, count: total })),
      );
    },
    filterCatalogue: (kind) => Promise.resolve(syntheticCatalogue(kind)),
    facetValues: (query, id) => {
      const counts = new Map<string, FacetValue>();
      for (const row of rowsFor(query)) {
        const owned = ownedOf(row, id);
        if (owned?.t !== "list") continue;
        for (const value of new Set(owned.values.map((v) => v.toLowerCase()))) {
          const name = owned.values.find((v) => v.toLowerCase() === value) ?? value;
          const entry = counts.get(value) ?? { value: name, label: name, count: 0 };
          entry.count += 1;
          counts.set(value, entry);
        }
      }
      const values = [...counts.values()].sort(
        (a, b) => b.count - a.count || collator.compare(a.value, b.value),
      );
      return Promise.resolve(values);
    },
    toggleWatched: (id) => {
      const current = watched.get(id) ?? syntheticCard(Number(id) - 1).watched;
      const previous = [stateOf(id, current)];
      watched.set(id, !current);
      watchedVersion += 1;
      return Promise.resolve({ watched: !current, previous });
    },
    setWatched: (ids, flag) => {
      const previous = [];
      for (const id of ids) {
        const current = watched.get(id) ?? syntheticCard(Number(id) - 1).watched;
        if (current === flag) continue;
        previous.push(stateOf(id, current));
        watched.set(id, flag);
        watchedVersion += 1;
      }
      return Promise.resolve(previous);
    },
    restoreWatched: (previous) => {
      for (const state of previous) watched.set(state.id, state.watched);
      watchedVersion += 1;
      return Promise.resolve();
    },
  };
}
