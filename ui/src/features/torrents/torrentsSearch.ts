/** The kinds of title the Torrent search page can look up. */
export type TitleKind = "movie" | "show";

/**
 * The search of `/torrents`.
 *
 * - `q`: the text in the title field, while no title is confirmed.
 * - `title`: the confirmed title, `movie:123` or `show:456` (a TMDB ID).
 * - `season`, `episode`: the scope of a show; none is the whole show. An episode needs its season.
 * - `only`: the library chips that are on, `better`, `gaps` or `better,gaps`; needs a title.
 */
export interface TorrentsSearch {
  q?: string;
  title?: string;
  season?: number;
  episode?: number;
  only?: string;
}

/** The library chips: releases better than the copy, and releases that fill a show's gaps. */
export type LibraryFilter = "better" | "gaps";

const LIBRARY_FILTERS: readonly LibraryFilter[] = ["better", "gaps"];

/** The library chips an `only` parameter turns on; anything else is dropped. */
export function parseOnly(only: string | undefined): ReadonlySet<LibraryFilter> {
  const words = (only ?? "").split(",").map((word) => word.trim());
  return new Set(LIBRARY_FILTERS.filter((filter) => words.includes(filter)));
}

/** The `only` parameter of these chips, or undefined when none is on. */
export function onlyParam(filters: ReadonlySet<LibraryFilter>): string | undefined {
  const on = LIBRARY_FILTERS.filter((filter) => filters.has(filter));
  return on.length === 0 ? undefined : on.join(",");
}

/** A confirmed title. */
export interface TitleRef {
  kind: TitleKind;
  tmdbId: number;
}

const TITLE = /^(movie|show):([1-9]\d{0,9})$/;

/** The parameter of a title: `movie:123`. */
export function titleParam(ref: TitleRef): string {
  return `${ref.kind}:${ref.tmdbId}`;
}

/** The title a `title` parameter names, or `null` when it is missing or damaged. */
export function parseTitleRef(title: string | undefined): TitleRef | null {
  if (title === undefined) return null;
  const match = TITLE.exec(title);
  if (match === null) return null;
  return { kind: match[1] === "show" ? "show" : "movie", tmdbId: Number(match[2]) };
}

function wholeNumber(value: unknown, min: number, max: number): number | undefined {
  const number =
    typeof value === "number" ? value : typeof value === "string" ? Number(value.trim()) : NaN;
  return Number.isInteger(number) && number >= min && number <= max ? number : undefined;
}

/**
 * Reads the search of `/torrents`. Anything damaged is dropped: a title that is not `movie:` or
 * `show:` and a number, a season on a movie, an episode without a season.
 */
export function validateTorrentsSearch(search: Record<string, unknown>): TorrentsSearch {
  const q = typeof search.q === "string" || typeof search.q === "number" ? String(search.q) : "";
  const ref = parseTitleRef(typeof search.title === "string" ? search.title : undefined);
  const season = ref?.kind === "show" ? wholeNumber(search.season, 0, 999) : undefined;
  const episode = season === undefined ? undefined : wholeNumber(search.episode, 1, 9999);
  const only =
    ref === null
      ? undefined
      : onlyParam(parseOnly(typeof search.only === "string" ? search.only : undefined));
  return {
    ...(q.trim() === "" ? {} : { q }),
    ...(ref === null ? {} : { title: titleParam(ref) }),
    ...(season === undefined ? {} : { season }),
    ...(episode === undefined ? {} : { episode }),
    ...(only === undefined ? {} : { only }),
  };
}

/** The link that opens Torrent search with a title (and a scope) confirmed. */
export function torrentsLink(search: TorrentsSearch): string {
  const clean = validateTorrentsSearch({ ...search });
  const params = new URLSearchParams();
  if (clean.q !== undefined) params.set("q", clean.q);
  if (clean.title !== undefined) params.set("title", clean.title);
  if (clean.season !== undefined) params.set("season", String(clean.season));
  if (clean.episode !== undefined) params.set("episode", String(clean.episode));
  if (clean.only !== undefined) params.set("only", clean.only);
  const text = params.toString();
  return text === "" ? "/torrents" : `/torrents?${text}`;
}

/** What is searched for within a title. */
export type ScopeChoice =
  | { kind: "all" }
  | { kind: "season"; season: number }
  | { kind: "episode"; season: number; episode: number };

/** The scope a search carries: the whole show, a season or an episode. */
export function scopeOf(search: TorrentsSearch): ScopeChoice {
  if (search.season === undefined) return { kind: "all" };
  if (search.episode === undefined) return { kind: "season", season: search.season };
  return { kind: "episode", season: search.season, episode: search.episode };
}

/** The TMDB ID among a title's provider IDs, or null when it has none (or a damaged one). */
export function tmdbIdOf(ids: readonly { provider: string; value: string }[]): number | null {
  const value = ids.find((id) => id.provider === "tmdb")?.value.trim() ?? "";
  return /^[1-9]\d{0,9}$/.test(value) ? Number(value) : null;
}
