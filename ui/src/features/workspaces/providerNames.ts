/** The names people know providers by; a key this build does not list is shown as it is. */
const NAMES: Readonly<Record<string, string>> = {
  tmdb: "TMDB",
  tvdb: "TheTVDB",
  imdb: "IMDb",
  omdb: "OMDb",
  trakt: "Trakt",
  tvmaze: "TVmaze",
  anidb: "AniDB",
  fanart: "Fanart.tv",
  fanarttv: "Fanart.tv",
  frames: "Frames",
  ofdb: "OFDb",
  moviemeter: "MovieMeter",
  mpdb: "MPDb.tv",
  fernsehserien: "fernsehserien.de",
  imdbdataset: "IMDb ratings dataset",
  mdblist: "MDBList",
  thesportsdb: "TheSportsDB",
  youtube: "YouTube",
  search: "Search",
  movies: "Movies",
  shows: "Shows",
};

/** A provider's display name: "TMDB", "Fanart.tv"; the key itself when it is not known. */
export function providerName(key: string): string {
  return NAMES[key] ?? key;
}
