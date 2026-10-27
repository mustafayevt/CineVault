/**
 * Where a click on a rating goes: the title's page on that source's site.
 *
 * A page the source named itself (stored with the rating, for example the Rotten Tomatoes page
 * MDBList or OMDb gave) wins. Otherwise the address is built from the title's IDs, and for the
 * sites that have no ID-based address, a search on the site by title and year. A source with
 * neither has no page (`null`) and its rating is not a link.
 */

/** The title a rating belongs to, as the page knows it. */
export interface RatingLinkTarget {
  kind: "movie" | "show" | "season" | "episode";
  /** The title's own provider IDs (`imdb`, `tmdb`, `trakt`, `tvdb`, ...). */
  ids: Readonly<Record<string, string>>;
  title: string;
  /** The year, or 0 when unknown. */
  year: number;
  /** For a season or an episode: the show's IDs and title. */
  show?: { ids: Readonly<Record<string, string>>; title: string };
  /** The season number of a season or an episode. */
  season?: number;
  /** The episode number of an episode. */
  episode?: number;
}

/** A list of `{ provider, value }` IDs as a lookup by provider. */
export function idMap(
  ids: readonly { provider: string; value: string }[],
): Readonly<Record<string, string>> {
  const out: Record<string, string> = {};
  for (const { provider, value } of ids) {
    const trimmed = value.trim();
    if (trimmed !== "" && out[provider] === undefined) out[provider] = trimmed;
  }
  return out;
}

const enc = encodeURIComponent;

function imdbId(ids: Readonly<Record<string, string>> | undefined): string | null {
  const id = ids?.imdb ?? "";
  return /^tt\d+$/.test(id) ? id : null;
}

function numericId(ids: Readonly<Record<string, string>> | undefined, key: string): string | null {
  const id = ids?.[key] ?? "";
  return /^\d+$/.test(id) ? id : null;
}

/** Whether a stored page address may be opened: an absolute `http(s)` address with a host. */
export function isWebAddress(url: string): boolean {
  const match = /^https?:\/\/([^/?#\s]+)/i.exec(url.trim());
  const host = match?.[1] ?? "";
  return host !== "" && !host.includes("@") && !/\s/.test(url.trim());
}

/** The words a site search looks for: the title and, when known, the year. */
function searchWords(target: RatingLinkTarget): string {
  const title =
    target.kind === "movie" || target.kind === "show"
      ? target.title
      : (target.show?.title ?? target.title);
  return target.year > 0 && (target.kind === "movie" || target.kind === "show")
    ? `${title} ${target.year}`
    : title;
}

function isShowLike(target: RatingLinkTarget): boolean {
  return target.kind !== "movie";
}

function imdbPage(target: RatingLinkTarget): string | null {
  const own = imdbId(target.ids);
  if (own !== null && target.kind !== "season") return `https://www.imdb.com/title/${own}/`;
  const show = imdbId(target.show?.ids);
  if (show === null) return null;
  return target.kind === "season" && target.season !== undefined
    ? `https://www.imdb.com/title/${show}/episodes/?season=${target.season}`
    : `https://www.imdb.com/title/${show}/`;
}

function tmdbPage(target: RatingLinkTarget): string | null {
  if (target.kind === "movie") {
    const id = numericId(target.ids, "tmdb");
    return id === null ? null : `https://www.themoviedb.org/movie/${id}`;
  }
  if (target.kind === "show") {
    const id = numericId(target.ids, "tmdb");
    return id === null ? null : `https://www.themoviedb.org/tv/${id}`;
  }
  // A season or an episode lives under its show's TMDB ID.
  const show = numericId(target.show?.ids, "tmdb");
  if (show === null || target.season === undefined) return null;
  const season = `https://www.themoviedb.org/tv/${show}/season/${target.season}`;
  if (target.kind === "season") return season;
  return target.episode === undefined ? null : `${season}/episode/${target.episode}`;
}

function traktPage(target: RatingLinkTarget): string | null {
  const ids = target.kind === "season" ? target.show?.ids : target.ids;
  const type = target.kind === "movie" ? "movie" : target.kind === "episode" ? "episode" : "show";
  const imdb = imdbId(ids);
  if (imdb !== null) return `https://trakt.tv/search/imdb/${imdb}`;
  const tmdb = numericId(ids, "tmdb");
  if (tmdb !== null) return `https://trakt.tv/search/tmdb/${tmdb}?id_type=${type}`;
  const trakt = numericId(ids, "trakt");
  if (trakt !== null) return `https://trakt.tv/search/trakt/${trakt}?id_type=${type}`;
  return null;
}

function letterboxdPage(target: RatingLinkTarget): string | null {
  // Letterboxd lists films only.
  if (target.kind !== "movie") return null;
  const imdb = imdbId(target.ids);
  if (imdb !== null) return `https://letterboxd.com/imdb/${imdb}/`;
  const tmdb = numericId(target.ids, "tmdb");
  return tmdb === null ? null : `https://letterboxd.com/tmdb/${tmdb}/`;
}

function rottenTomatoesSearch(target: RatingLinkTarget): string | null {
  if (target.title.trim() === "") return null;
  return `https://www.rottentomatoes.com/search?search=${enc(searchWords(target))}`;
}

function metacriticSearch(target: RatingLinkTarget): string | null {
  if (target.title.trim() === "") return null;
  const category = isShowLike(target) ? 1 : 2;
  return `https://www.metacritic.com/search/${enc(searchWords(target))}/?category=${category}`;
}

function rogerEbertSearch(target: RatingLinkTarget): string | null {
  if (target.kind !== "movie" || target.title.trim() === "") return null;
  return `https://www.rogerebert.com/search?q=${enc(searchWords(target))}`;
}

function myAnimeListPage(target: RatingLinkTarget): string | null {
  const ids = target.kind === "season" || target.kind === "episode" ? target.show?.ids : target.ids;
  const id = numericId(ids, "myanimelist") ?? numericId(ids, "mal");
  if (id !== null) return `https://myanimelist.net/anime/${id}`;
  if (target.title.trim() === "") return null;
  return `https://myanimelist.net/anime.php?q=${enc(searchWords(target))}`;
}

function tvdbPage(target: RatingLinkTarget): string | null {
  if (target.kind === "movie") {
    const id = numericId(target.ids, "tvdb");
    return id === null ? null : `https://thetvdb.com/dereferrer/movie/${id}`;
  }
  if (target.kind === "episode") {
    const id = numericId(target.ids, "tvdb");
    if (id !== null) return `https://thetvdb.com/dereferrer/episode/${id}`;
  }
  const id = numericId(target.kind === "show" ? target.ids : target.show?.ids, "tvdb");
  return id === null ? null : `https://thetvdb.com/dereferrer/series/${id}`;
}

function tvmazePage(target: RatingLinkTarget): string | null {
  if (target.kind === "movie") return null;
  if (target.kind === "episode") {
    const id = numericId(target.ids, "tvmaze");
    if (id !== null) return `https://www.tvmaze.com/episodes/${id}`;
  }
  const id = numericId(target.kind === "show" ? target.ids : target.show?.ids, "tvmaze");
  return id === null ? null : `https://www.tvmaze.com/shows/${id}`;
}

function aniDbPage(target: RatingLinkTarget): string | null {
  const id = numericId(
    target.kind === "show" || target.kind === "movie" ? target.ids : target.show?.ids,
    "anidb",
  );
  return id === null ? null : `https://anidb.net/anime/${id}`;
}

function movieMeterPage(target: RatingLinkTarget): string | null {
  if (target.kind !== "movie") return null;
  const id = numericId(target.ids, "moviemeter");
  return id === null ? null : `https://www.moviemeter.nl/film/${id}`;
}

const BUILDERS: Readonly<Record<string, (target: RatingLinkTarget) => string | null>> = {
  imdb: imdbPage,
  tmdb: tmdbPage,
  trakt: traktPage,
  letterboxd: letterboxdPage,
  tomatometerallcritics: rottenTomatoesSearch,
  tomatometerallaudience: rottenTomatoesSearch,
  tomatometeravgcritics: rottenTomatoesSearch,
  tomatometeravgaudience: rottenTomatoesSearch,
  metacritic: metacriticSearch,
  metacriticuser: metacriticSearch,
  rogerebert: rogerEbertSearch,
  myanimelist: myAnimeListPage,
  tvdb: tvdbPage,
  tvmaze: tvmazePage,
  anidb: aniDbPage,
  moviemeter: movieMeterPage,
};

/**
 * The page a click on the rating of `source` opens, or `null` when there is none. `stored` is
 * the address the source named itself, kept with the rating; it wins when it is a web address.
 */
export function ratingPageUrl(
  source: string,
  target: RatingLinkTarget,
  stored?: string | null,
): string | null {
  if (stored !== undefined && stored !== null && isWebAddress(stored)) return stored.trim();
  const key = source.toLowerCase();
  const build = Object.hasOwn(BUILDERS, key) ? BUILDERS[key] : undefined;
  return build === undefined ? null : build(target);
}
