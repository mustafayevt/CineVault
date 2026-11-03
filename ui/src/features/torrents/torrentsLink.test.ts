import { describe, expect, it } from "vitest";

import { parseOnly, tmdbIdOf, torrentsLink, validateTorrentsSearch } from "./torrentsSearch";

describe("the Torrents link", () => {
  it("opens the search at a title, a season and an episode", () => {
    expect(torrentsLink({ title: "show:95396", season: 2, episode: 5 })).toBe(
      "/torrents?title=show%3A95396&season=2&episode=5",
    );
    expect(torrentsLink({ title: "movie:693134" })).toBe("/torrents?title=movie%3A693134");
  });
});

describe("the library chips in the address", () => {
  it("keeps the known chips of a title and drops the rest", () => {
    expect(torrentsLink({ title: "movie:1", only: "better" })).toBe(
      "/torrents?title=movie%3A1&only=better",
    );
    expect(validateTorrentsSearch({ title: "show:2", only: "gaps,better,nope" })).toEqual({
      title: "show:2",
      only: "better,gaps",
    });
    expect(validateTorrentsSearch({ only: "better" })).toEqual({});
    expect([...parseOnly("gaps")]).toEqual(["gaps"]);
    expect(parseOnly(undefined).size).toBe(0);
  });

  it("reads the TMDB ID of a title", () => {
    expect(
      tmdbIdOf([
        { provider: "imdb", value: "tt1" },
        { provider: "tmdb", value: " 603 " },
      ]),
    ).toBe(603);
    expect(tmdbIdOf([{ provider: "tmdb", value: "abc" }])).toBeNull();
    expect(tmdbIdOf([])).toBeNull();
  });
});
