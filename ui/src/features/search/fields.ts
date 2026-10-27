import type { LibraryChipFieldDto } from "../../bindings";
import type { MessageKey } from "../../i18n/catalog";

/** The field a chip tests; the same names the backend uses. */
export type ChipField = LibraryChipFieldDto;

/** How a field's value is written in the quick search. */
export type FieldKind =
  /** Yes or no: `-new`, `is:review`, `new:no`. */
  | "flag"
  /** A whole number or range: `year:2010..2019`, `runtime:>120`. */
  | "integer"
  /** A number with one decimal or a range: `rating:7.5..`. */
  | "decimal"
  /** One or more words, any of which matches: `genre:drama,"sci-fi"`. */
  | "list";

export interface FieldDef {
  id: ChipField;
  /** What can be typed before the colon, lower case. The first is the canonical name. */
  names: readonly string[];
  kind: FieldKind;
  /** The chip's name for the field. */
  label: MessageKey;
  /** Smallest and largest number a range may use (integer and decimal fields). */
  bounds?: { min: number; max: number };
  /** Maps typed text to the stored value, or `null` when the text is not one. List fields only. */
  normalize?: (text: string) => string | null;
}

const HDR_FORMATS: Readonly<Record<string, string>> = {
  dv: "Dolby Vision",
  dovi: "Dolby Vision",
  dolbyvision: "Dolby Vision",
  dolby: "Dolby Vision",
  hdr10: "HDR10",
  "hdr10+": "HDR10+",
  hdr10plus: "HDR10+",
  hlg: "HLG",
};

/** The stored name of an HDR format typed in any common spelling. */
function normalizeHdr(text: string): string | null {
  const key = text.toLowerCase().replaceAll(/[\s_-]/g, "");
  return HDR_FORMATS[key] ?? null;
}

const RESOLUTIONS: Readonly<Record<string, string>> = {
  "4k": "2160p",
  uhd: "2160p",
  "2160": "2160p",
  "2160p": "2160p",
  fhd: "1080p",
  "1080": "1080p",
  "1080p": "1080p",
  hd: "720p",
  "720": "720p",
  "720p": "720p",
  "576": "576p",
  "576p": "576p",
  sd: "480p",
  "480": "480p",
  "480p": "480p",
};

/** The resolution class (as shown on a poster) for a typed resolution. */
function normalizeResolution(text: string): string | null {
  return RESOLUTIONS[text.toLowerCase().trim()] ?? null;
}

const STAGES: Readonly<Record<string, string>> = {
  new: "new",
  scraped: "scraped",
  renamed: "renamed",
};

/** The stage a typed word stands for (`new`, `scraped`, `renamed`), in any case. */
function normalizeStage(text: string): string | null {
  return STAGES[text.toLowerCase().trim()] ?? null;
}

const notBlank = (text: string): string | null => (text.trim() === "" ? null : text.trim());

const FIELDS: readonly FieldDef[] = [
  { id: "new", names: ["new"], kind: "flag", label: "search.field.new" },
  // The lifecycle flags, as the pipeline counts count them, so a count opens exactly its titles.
  { id: "review", names: ["review", "needsreview"], kind: "flag", label: "search.field.review" },
  {
    id: "not_matched",
    names: ["notmatched", "unmatched"],
    kind: "flag",
    label: "search.field.notMatched",
  },
  {
    id: "rename_pending",
    names: ["renamepending", "pending"],
    kind: "flag",
    label: "search.field.renamePending",
  },
  { id: "missing", names: ["missing", "offline"], kind: "flag", label: "search.field.missing" },
  { id: "locked", names: ["locked"], kind: "flag", label: "search.field.locked" },
  {
    id: "year",
    names: ["year"],
    kind: "integer",
    label: "search.field.year",
    bounds: { min: 1000, max: 9999 },
  },
  {
    id: "runtime",
    names: ["runtime", "minutes"],
    kind: "integer",
    label: "search.field.runtime",
    bounds: { min: 0, max: 100_000 },
  },
  {
    id: "top250",
    names: ["top250", "top"],
    kind: "integer",
    label: "search.field.top250",
    bounds: { min: 1, max: 250 },
  },
  {
    id: "rating",
    names: ["rating"],
    kind: "decimal",
    label: "search.field.rating",
    bounds: { min: 0, max: 10 },
  },
  {
    id: "imdb_rating",
    names: ["imdb"],
    kind: "decimal",
    label: "search.field.imdbRating",
    bounds: { min: 0, max: 10 },
  },
  {
    id: "tmdb_rating",
    names: ["tmdb"],
    kind: "decimal",
    label: "search.field.tmdbRating",
    bounds: { min: 0, max: 10 },
  },
  {
    id: "rotten_tomatoes_rating",
    names: ["rottentomatoes", "rt", "tomatoes"],
    kind: "decimal",
    label: "search.field.rottenTomatoesRating",
    bounds: { min: 0, max: 10 },
  },
  {
    id: "metacritic_rating",
    names: ["metacritic", "mc"],
    kind: "decimal",
    label: "search.field.metacriticRating",
    bounds: { min: 0, max: 10 },
  },
  {
    id: "user_rating",
    names: ["myrating", "userrating"],
    kind: "decimal",
    label: "search.field.userRating",
    bounds: { min: 0, max: 10 },
  },
  {
    id: "resolution",
    names: ["resolution", "res"],
    kind: "list",
    label: "search.field.resolution",
    normalize: normalizeResolution,
  },
  { id: "hdr", names: ["hdr"], kind: "list", label: "search.field.hdr", normalize: normalizeHdr },
  {
    id: "codec",
    names: ["codec", "videocodec"],
    kind: "list",
    label: "search.field.codec",
    normalize: notBlank,
  },
  {
    id: "audio_language",
    names: ["audio", "audiolanguage"],
    kind: "list",
    label: "search.field.audioLanguage",
    normalize: notBlank,
  },
  {
    id: "subtitle_language",
    names: ["subs", "subtitles", "subtitle", "sub"],
    kind: "list",
    label: "search.field.subtitleLanguage",
    normalize: notBlank,
  },
  {
    id: "genre",
    names: ["genre", "genres"],
    kind: "list",
    label: "search.field.genre",
    normalize: notBlank,
  },
  {
    id: "actor",
    names: ["actor", "actors", "cast"],
    kind: "list",
    label: "search.field.actor",
    normalize: notBlank,
  },
  {
    id: "director",
    names: ["director", "directors"],
    kind: "list",
    label: "search.field.director",
    normalize: notBlank,
  },
  {
    id: "writer",
    names: ["writer", "writers"],
    kind: "list",
    label: "search.field.writer",
    normalize: notBlank,
  },
  {
    id: "certification",
    names: ["certification", "cert", "rated"],
    kind: "list",
    label: "search.field.certification",
    normalize: notBlank,
  },
  {
    id: "edition",
    names: ["edition"],
    kind: "list",
    label: "search.field.edition",
    normalize: notBlank,
  },
  // The release the file came from (Blu-ray, WEB-DL ...).
  {
    id: "source",
    names: ["release", "mediasource"],
    kind: "list",
    label: "search.field.source",
    normalize: notBlank,
  },
  // The library folder a title lives on, by its label ("NAS 4K").
  {
    id: "library_source",
    names: ["source", "folder"],
    kind: "list",
    label: "search.field.libraryFolder",
    normalize: notBlank,
  },
  // Where a title is in the pipeline: New, Scraped or Renamed.
  {
    id: "stage",
    names: ["stage"],
    kind: "list",
    label: "search.field.stage",
    normalize: normalizeStage,
  },
  // Words a title contains, as the renamer's ${title}, ${originalTitle} and ${sortTitle} tokens.
  { id: "title", names: ["title"], kind: "list", label: "search.field.title", normalize: notBlank },
  {
    id: "original_title",
    names: ["originaltitle", "original"],
    kind: "list",
    label: "search.field.originalTitle",
    normalize: notBlank,
  },
  {
    id: "sort_title",
    names: ["sorttitle"],
    kind: "list",
    label: "search.field.sortTitle",
    normalize: notBlank,
  },
];

const BY_NAME = new Map<string, FieldDef>(
  FIELDS.flatMap((def) => def.names.map((name) => [name, def] as const)),
);

const BY_ID = new Map<ChipField, FieldDef>(FIELDS.map((def) => [def.id, def]));

/** The field a typed name stands for, ignoring case; `undefined` for an unknown name. */
export function fieldByName(name: string): FieldDef | undefined {
  return BY_NAME.get(name.toLowerCase());
}

export function fieldById(id: ChipField): FieldDef {
  const def = BY_ID.get(id);
  if (def === undefined) throw new Error(`unknown chip field ${id}`);
  return def;
}
