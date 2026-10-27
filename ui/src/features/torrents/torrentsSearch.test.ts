import { describe, expect, it } from "vitest";

import {
  parseTitleRef,
  scopeOf,
  titleParam,
  torrentsLink,
  validateTorrentsSearch,
} from "./torrentsSearch";

describe("the search of /torrents", () => {
  it("keeps a title, a scope and the typed text", () => {
    expect(validateTorrentsSearch({ title: "show:456", season: 1, episode: 2, q: "sev" })).toEqual({
      q: "sev",
      title: "show:456",
      season: 1,
      episode: 2,
    });
    expect(validateTorrentsSearch({ title: "movie:123" })).toEqual({ title: "movie:123" });
  });

  it("reads numbers the router parsed and numbers that arrived as text", () => {
    expect(validateTorrentsSearch({ title: "show:5", season: "2", episode: "3" })).toEqual({
      title: "show:5",
      season: 2,
      episode: 3,
    });
    expect(validateTorrentsSearch({ q: 1917 })).toEqual({ q: "1917" });
  });

  it("drops what is damaged", () => {
    for (const title of ["movie", "movie:", "movie:0", "show:-1", "tv:5", "movie:1x", ":9", 7]) {
      expect(validateTorrentsSearch({ title }), String(title)).toEqual({});
    }
    expect(validateTorrentsSearch({ q: "   " })).toEqual({});
    expect(validateTorrentsSearch({ q: {} })).toEqual({});
  });

  it("allows a scope only on a show, and an episode only with its season", () => {
    expect(validateTorrentsSearch({ title: "movie:1", season: 1, episode: 2 })).toEqual({
      title: "movie:1",
    });
    expect(validateTorrentsSearch({ title: "show:1", episode: 2 })).toEqual({ title: "show:1" });
    expect(validateTorrentsSearch({ season: 1 })).toEqual({});
    expect(validateTorrentsSearch({ title: "show:1", season: -1 })).toEqual({ title: "show:1" });
    expect(validateTorrentsSearch({ title: "show:1", season: 1, episode: 0 })).toEqual({
      title: "show:1",
      season: 1,
    });
    expect(validateTorrentsSearch({ title: "show:1", season: 0, episode: 1 })).toEqual({
      title: "show:1",
      season: 0,
      episode: 1,
    });
    expect(validateTorrentsSearch({ title: "show:1", season: 1.5 })).toEqual({ title: "show:1" });
  });

  it("reads and writes a title reference", () => {
    expect(parseTitleRef("movie:603")).toEqual({ kind: "movie", tmdbId: 603 });
    expect(parseTitleRef("show:1396")).toEqual({ kind: "show", tmdbId: 1396 });
    expect(parseTitleRef(undefined)).toBeNull();
    expect(parseTitleRef("movie:abc")).toBeNull();
    expect(titleParam({ kind: "show", tmdbId: 9 })).toBe("show:9");
  });

  it("builds the link the detail pages use", () => {
    expect(torrentsLink({})).toBe("/torrents");
    expect(torrentsLink({ title: "movie:123" })).toBe("/torrents?title=movie%3A123");
    expect(torrentsLink({ title: "show:456", season: 1, episode: 2 })).toBe(
      "/torrents?title=show%3A456&season=1&episode=2",
    );
    // A damaged search never becomes a damaged link.
    expect(torrentsLink({ title: "tv:1", season: 3 })).toBe("/torrents");
  });

  it("names the scope a search carries", () => {
    expect(scopeOf({ title: "show:1" })).toEqual({ kind: "all" });
    expect(scopeOf({ title: "show:1", season: 2 })).toEqual({ kind: "season", season: 2 });
    expect(scopeOf({ title: "show:1", season: 2, episode: 5 })).toEqual({
      kind: "episode",
      season: 2,
      episode: 5,
    });
  });
});
