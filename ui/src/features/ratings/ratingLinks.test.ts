import { describe, expect, it } from "vitest";

import { idMap, isWebAddress, ratingPageUrl } from "./ratingLinks";
import type { RatingLinkTarget } from "./ratingLinks";

const MOVIE: RatingLinkTarget = {
  kind: "movie",
  ids: { imdb: "tt0083658", tmdb: "78", trakt: "48" },
  title: "Blade Runner",
  year: 1982,
};
const BARE_MOVIE: RatingLinkTarget = { kind: "movie", ids: {}, title: "Blade Runner", year: 1982 };
const SHOW: RatingLinkTarget = {
  kind: "show",
  ids: { imdb: "tt0903747", tmdb: "1396", tvdb: "81189", tvmaze: "169" },
  title: "Breaking Bad",
  year: 2008,
};
const EPISODE: RatingLinkTarget = {
  kind: "episode",
  ids: { imdb: "tt0959621", tmdb: "62085" },
  title: "Pilot",
  year: 0,
  show: { ids: { imdb: "tt0903747", tmdb: "1396" }, title: "Breaking Bad" },
  season: 1,
  episode: 1,
};

describe("rating page addresses", () => {
  it("builds the IMDb, TMDB, Trakt and Letterboxd pages of a movie from its IDs", () => {
    expect(ratingPageUrl("imdb", MOVIE)).toBe("https://www.imdb.com/title/tt0083658/");
    expect(ratingPageUrl("tmdb", MOVIE)).toBe("https://www.themoviedb.org/movie/78");
    expect(ratingPageUrl("trakt", MOVIE)).toBe("https://trakt.tv/search/imdb/tt0083658");
    expect(ratingPageUrl("letterboxd", MOVIE)).toBe("https://letterboxd.com/imdb/tt0083658/");
  });

  it("falls back to the TMDB or Trakt ID when there is no IMDb ID", () => {
    const tmdbOnly = { ...MOVIE, ids: { tmdb: "78" } };
    expect(ratingPageUrl("trakt", tmdbOnly)).toBe("https://trakt.tv/search/tmdb/78?id_type=movie");
    expect(ratingPageUrl("letterboxd", tmdbOnly)).toBe("https://letterboxd.com/tmdb/78/");
    expect(ratingPageUrl("imdb", tmdbOnly)).toBeNull();
    expect(ratingPageUrl("trakt", { ...MOVIE, ids: { trakt: "48" } })).toBe(
      "https://trakt.tv/search/trakt/48?id_type=movie",
    );
  });

  it("has no ID page without the IDs, and no link for a source it cannot reach", () => {
    for (const source of ["imdb", "tmdb", "trakt", "letterboxd", "tvdb", "moviemeter"]) {
      expect(ratingPageUrl(source, BARE_MOVIE), source).toBeNull();
    }
    expect(ratingPageUrl("ofdb", MOVIE)).toBeNull();
    expect(ratingPageUrl("somenewsite", MOVIE)).toBeNull();
    // A malformed ID is not put into an address.
    expect(ratingPageUrl("imdb", { ...MOVIE, ids: { imdb: "nm0000148" } })).toBeNull();
    expect(ratingPageUrl("tmdb", { ...MOVIE, ids: { tmdb: "78/../x" } })).toBeNull();
  });

  it("searches Rotten Tomatoes, Metacritic and Roger Ebert by title and year without a stored page", () => {
    expect(ratingPageUrl("tomatometerallcritics", BARE_MOVIE)).toBe(
      "https://www.rottentomatoes.com/search?search=Blade%20Runner%201982",
    );
    expect(ratingPageUrl("tomatometerallaudience", BARE_MOVIE)).toBe(
      "https://www.rottentomatoes.com/search?search=Blade%20Runner%201982",
    );
    expect(ratingPageUrl("metacritic", BARE_MOVIE)).toBe(
      "https://www.metacritic.com/search/Blade%20Runner%201982/?category=2",
    );
    expect(ratingPageUrl("metacriticuser", SHOW)).toBe(
      "https://www.metacritic.com/search/Breaking%20Bad%202008/?category=1",
    );
    expect(ratingPageUrl("rogerebert", BARE_MOVIE)).toBe(
      "https://www.rogerebert.com/search?q=Blade%20Runner%201982",
    );
    expect(ratingPageUrl("metacritic", { ...BARE_MOVIE, title: " " })).toBeNull();
  });

  it("prefers the page the source named itself, when it is a web address", () => {
    const page = "https://www.rottentomatoes.com/m/blade_runner";
    expect(ratingPageUrl("tomatometerallcritics", MOVIE, page)).toBe(page);
    expect(ratingPageUrl("imdb", MOVIE, "file:///C:/Windows/notepad.exe")).toBe(
      "https://www.imdb.com/title/tt0083658/",
    );
    expect(ratingPageUrl("metacritic", MOVIE, "javascript:alert(1)")).toContain(
      "https://www.metacritic.com/search/",
    );
    expect(ratingPageUrl("letterboxd", MOVIE, null)).toBe("https://letterboxd.com/imdb/tt0083658/");
  });

  it("links a show to its TV pages and an episode to its own pages under the show", () => {
    expect(ratingPageUrl("tmdb", SHOW)).toBe("https://www.themoviedb.org/tv/1396");
    expect(ratingPageUrl("trakt", SHOW)).toBe("https://trakt.tv/search/imdb/tt0903747");
    expect(ratingPageUrl("tvdb", SHOW)).toBe("https://thetvdb.com/dereferrer/series/81189");
    expect(ratingPageUrl("tvmaze", SHOW)).toBe("https://www.tvmaze.com/shows/169");
    expect(ratingPageUrl("letterboxd", SHOW)).toBeNull();

    expect(ratingPageUrl("imdb", EPISODE)).toBe("https://www.imdb.com/title/tt0959621/");
    expect(ratingPageUrl("tmdb", EPISODE)).toBe(
      "https://www.themoviedb.org/tv/1396/season/1/episode/1",
    );
    // Without the show's TMDB ID the episode has no TMDB page.
    expect(
      ratingPageUrl("tmdb", { ...EPISODE, show: { ids: {}, title: "Breaking Bad" } }),
    ).toBeNull();
    // An episode without an IMDb ID of its own goes to the show's.
    expect(ratingPageUrl("imdb", { ...EPISODE, ids: {} })).toBe(
      "https://www.imdb.com/title/tt0903747/",
    );
    const season: RatingLinkTarget = { ...EPISODE, kind: "season", ids: {} };
    expect(ratingPageUrl("tmdb", season)).toBe("https://www.themoviedb.org/tv/1396/season/1");
    expect(ratingPageUrl("imdb", season)).toBe(
      "https://www.imdb.com/title/tt0903747/episodes/?season=1",
    );
  });

  it("reads IDs from the detail list and accepts only web addresses as stored pages", () => {
    expect(
      idMap([
        { provider: "imdb", value: " tt1 " },
        { provider: "tmdb", value: "" },
        { provider: "imdb", value: "tt2" },
      ]),
    ).toEqual({ imdb: "tt1" });
    expect(isWebAddress("https://letterboxd.com/film/x/")).toBe(true);
    expect(isWebAddress("HTTP://example.org")).toBe(true);
    for (const bad of ["", "/m/x", "ftp://x", "https://", "https://a@b.c/", "mailto:a@b.c"]) {
      expect(isWebAddress(bad), bad).toBe(false);
    }
  });
});
