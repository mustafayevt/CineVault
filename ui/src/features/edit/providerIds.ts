import type { EditKind } from "./editBackend";

interface ProviderSite {
  label: string;
  /** The page of the title on the provider's site, or `null` when it has none for this kind. */
  url: (id: string, kind: EditKind) => string | null;
}

const enc = encodeURIComponent;

/** The providers whose IDs a title can carry, with the page each ID opens. */
const PROVIDER_SITES: Readonly<Record<string, ProviderSite>> = {
  tmdb: {
    label: "TMDB",
    url: (id, kind) => `https://www.themoviedb.org/${kind === "movie" ? "movie" : "tv"}/${enc(id)}`,
  },
  imdb: { label: "IMDb", url: (id) => `https://www.imdb.com/title/${enc(id)}/` },
  tvdb: {
    label: "TheTVDB",
    url: (id, kind) =>
      `https://thetvdb.com/dereferrer/${kind === "movie" ? "movie" : "series"}/${enc(id)}`,
  },
  trakt: {
    label: "Trakt",
    url: (id, kind) =>
      `https://trakt.tv/search/trakt/${enc(id)}?id_type=${kind === "movie" ? "movie" : "show"}`,
  },
  tvmaze: { label: "TVmaze", url: (id) => `https://www.tvmaze.com/shows/${enc(id)}` },
  anidb: { label: "AniDB", url: (id) => `https://anidb.net/anime/${enc(id)}` },
  wikidata: { label: "Wikidata", url: (id) => `https://www.wikidata.org/wiki/${enc(id)}` },
  tmdbSet: {
    label: "TMDB collection",
    url: (id) => `https://www.themoviedb.org/collection/${enc(id)}`,
  },
};

function siteOf(source: string): ProviderSite | undefined {
  return Object.hasOwn(PROVIDER_SITES, source) ? PROVIDER_SITES[source] : undefined;
}

/** The ID sources the editor offers, in the order people expect them. */
export const PROVIDER_ID_SOURCES: readonly string[] = Object.keys(PROVIDER_SITES);

/** A provider's name as people know it: `TMDB`, `IMDb`; an unknown source keeps its own name. */
export function providerIdLabel(source: string): string {
  return siteOf(source)?.label ?? source;
}

/** The address an ID opens, or `null` when the provider has no page for it. */
export function providerIdUrl(source: string, id: string, kind: EditKind): string | null {
  const trimmed = id.trim();
  if (trimmed === "") return null;
  return siteOf(source)?.url(trimmed, kind) ?? null;
}
