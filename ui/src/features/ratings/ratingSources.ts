/**
 * The rating sources the app knows: how each is named, drawn and written. Keys are the ones the
 * library stores (`cv_domain::rating_key`).
 *
 * The brand colours below are the only literal colours allowed in components (design system:
 * "RatingChip"): they belong to the sources, not to CineVault.
 */

/** How a value of a source is written. */
export type RatingStyle =
  /** `8.5`, `4.4`, `3.5`: one decimal on the source's own scale. */
  | "decimal"
  /** `92%`. */
  | "percent"
  /** `79`: a whole score out of 100 without a percent sign. */
  | "score";

export interface RatingSourceInfo {
  /** The stored key: `imdb`, `tomatometerallcritics`, ... */
  id: string;
  /** The short name in the badge: `IMDb`, `RT`, `MC`. */
  short: string;
  /** The full name, for tooltips and screen readers. */
  name: string;
  /** The top of the source's own scale. */
  max: number;
  style: RatingStyle;
  /** Badge colours; `null` for a source without a brand colour (drawn on neutral tokens). */
  brand: { background: string; foreground: string } | null;
}

const SOURCES: readonly RatingSourceInfo[] = [
  {
    id: "imdb",
    short: "IMDb",
    name: "IMDb",
    max: 10,
    style: "decimal",
    brand: { background: "#F5C518", foreground: "#111111" },
  },
  {
    id: "tmdb",
    short: "TMDB",
    name: "TMDB",
    max: 10,
    style: "decimal",
    brand: { background: "#0D9EB8", foreground: "#0B0A09" },
  },
  {
    id: "trakt",
    short: "Trakt",
    name: "Trakt",
    max: 10,
    style: "decimal",
    brand: { background: "#D8263A", foreground: "#FFFFFF" },
  },
  {
    id: "letterboxd",
    short: "LB",
    name: "Letterboxd",
    max: 5,
    style: "decimal",
    brand: { background: "#3A4651", foreground: "#FFFFFF" },
  },
  {
    id: "tomatometerallcritics",
    short: "RT",
    name: "Rotten Tomatoes critics",
    max: 100,
    style: "percent",
    brand: { background: "#E2412E", foreground: "#FFFFFF" },
  },
  {
    id: "tomatometerallaudience",
    short: "RTA",
    name: "Rotten Tomatoes audience",
    max: 100,
    style: "percent",
    brand: { background: "#FA320A", foreground: "#FFFFFF" },
  },
  {
    id: "metacritic",
    short: "MC",
    name: "Metacritic",
    max: 100,
    style: "score",
    brand: { background: "#4FB15C", foreground: "#0B0A09" },
  },
  {
    id: "metacriticuser",
    short: "MCU",
    name: "Metacritic users",
    max: 10,
    style: "decimal",
    brand: { background: "#4FB15C", foreground: "#0B0A09" },
  },
  {
    id: "myanimelist",
    short: "MAL",
    name: "MyAnimeList",
    max: 10,
    style: "decimal",
    brand: { background: "#2E51A2", foreground: "#FFFFFF" },
  },
  {
    id: "rogerebert",
    short: "RE",
    name: "Roger Ebert",
    max: 4,
    style: "decimal",
    brand: { background: "#C9302C", foreground: "#FFFFFF" },
  },
  {
    id: "tvdb",
    short: "TVDB",
    name: "TheTVDB",
    max: 10,
    style: "decimal",
    brand: { background: "#6CD491", foreground: "#0B0A09" },
  },
  {
    id: "tvmaze",
    short: "TVmaze",
    name: "TVmaze",
    max: 10,
    style: "decimal",
    brand: { background: "#3C948B", foreground: "#FFFFFF" },
  },
  {
    id: "moviemeter",
    short: "MM",
    name: "MovieMeter",
    max: 5,
    style: "decimal",
    brand: null,
  },
  {
    id: "tomatometeravgcritics",
    short: "RT avg",
    name: "Rotten Tomatoes average critics rating",
    max: 10,
    style: "decimal",
    brand: { background: "#E2412E", foreground: "#FFFFFF" },
  },
  {
    id: "tomatometeravgaudience",
    short: "RTA avg",
    name: "Rotten Tomatoes average audience rating",
    max: 5,
    style: "decimal",
    brand: { background: "#FA320A", foreground: "#FFFFFF" },
  },
  {
    id: "ofdb",
    short: "OFDb",
    name: "OFDb",
    max: 10,
    style: "decimal",
    brand: null,
  },
  {
    id: "anidb",
    short: "AniDB",
    name: "AniDB",
    max: 10,
    style: "decimal",
    brand: null,
  },
  {
    id: "mpdb",
    short: "MPDb",
    name: "MPDb.tv",
    max: 10,
    style: "decimal",
    brand: null,
  },
  {
    id: "fernsehserien",
    short: "FS",
    name: "fernsehserien.de",
    max: 10,
    style: "decimal",
    brand: null,
  },
];

/** Every source the app describes, in badge order: the choices of the ratings editor. */
export const KNOWN_RATING_SOURCES: readonly RatingSourceInfo[] = SOURCES;

/** The info of a stored source key, or `null` for one the app has no description of. */
export function ratingSource(id: string): RatingSourceInfo | null {
  const key = id.toLowerCase();
  return SOURCES.find((source) => source.id === key) ?? null;
}

/**
 * A key the app has no description of, written as a name: `somenewsite` reads "Somenewsite", never
 * a shouted code.
 */
function unknownName(id: string): string {
  const words = id.trim().replace(/[_-]+/g, " ");
  return words === "" ? id : words.charAt(0).toUpperCase() + words.slice(1);
}

/** The full name of a source; an unknown key is written as a capitalised word. */
export function ratingSourceName(id: string): string {
  return ratingSource(id)?.name ?? unknownName(id);
}

/**
 * A value as the source writes it. `value` is on the source's own scale (`max`): Letterboxd 4.4
 * out of 5, Rotten Tomatoes 92 out of 100. `digits` formats a plain number in the user's locale.
 */
export function formatRatingValue(
  id: string,
  value: number,
  max: number,
  digits: (n: number) => string,
): string {
  const style = ratingSource(id)?.style ?? (max === 100 ? "percent" : "decimal");
  switch (style) {
    case "percent":
      return `${digits(Math.round(value))}%`;
    case "score":
      return digits(Math.round(value));
    case "decimal":
      return digits(Math.round(value * 10) / 10);
  }
}

/** A value from the 0 to 10 scale the library rows use, on the source's own scale. */
export function toNativeScale(id: string, valueOnTen: number): number {
  const max = ratingSource(id)?.max ?? 10;
  return (valueOnTen * max) / 10;
}

/** The sources a person can prefer, in the order the settings list shows them. */
export const PREFERRABLE_SOURCES: readonly string[] = [
  "tmdb",
  "imdb",
  "trakt",
  "letterboxd",
  "tomatometerallcritics",
  "metacritic",
  "myanimelist",
  "tvdb",
];

/** The source used until a person picks one. */
export const DEFAULT_PREFERRED_SOURCE = "tmdb";

/**
 * The sources tried, in order, after the preferred one (`cv_domain::FALLBACK_ORDER`); sources not
 * listed come after these. Keep the two lists identical.
 */
export const FALLBACK_ORDER: readonly string[] = [
  "imdb",
  "tmdb",
  "trakt",
  "tvmaze",
  "tvdb",
  "myanimelist",
  "moviemeter",
  "letterboxd",
  "tomatometerallcritics",
  "metacritic",
  "tomatometerallaudience",
  "tomatometeravgcritics",
  "tomatometeravgaudience",
  "rogerebert",
];

/** The sources in the order they are tried for a preferred source. */
export function ratingOrder(preferred: string): readonly string[] {
  const first = preferred.trim().toLowerCase();
  return [first, ...FALLBACK_ORDER.filter((source) => source !== first)];
}

/** Where a source ranks for a preferred source: lower wins; unlisted sources come last. */
export function ratingRank(preferred: string, source: string): number {
  const order = ratingOrder(preferred);
  const index = order.indexOf(source.toLowerCase());
  return index === -1 ? order.length : index;
}

/** One rating of one source, on the source's own scale. */
export interface SourceRating {
  source: string;
  value: number;
  max: number;
}

/** What a title shows for a preferred source, and whether it stands in for it. */
export interface PickedRating<T extends SourceRating> {
  rating: T;
  fallback: boolean;
}

/**
 * The rating a title shows: the preferred source's, or else the first available source in the
 * fallback order. The user's own rating (`user`) never counts. Mirrors `Ratings::pick` in
 * `cv_domain`.
 */
export function pickRating<T extends SourceRating>(
  ratings: readonly T[],
  preferred: string,
): PickedRating<T> | null {
  let best: T | null = null;
  for (const rating of ratings) {
    if (rating.source === "user" || rating.max <= 0 || rating.value < 0) continue;
    if (
      best === null ||
      ratingRank(preferred, rating.source) < ratingRank(preferred, best.source) ||
      (ratingRank(preferred, rating.source) === ratingRank(preferred, best.source) &&
        rating.source < best.source)
    ) {
      best = rating;
    }
  }
  return best === null ? null : { rating: best, fallback: best.source !== preferred.toLowerCase() };
}
