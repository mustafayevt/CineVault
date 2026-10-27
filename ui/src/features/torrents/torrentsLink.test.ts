import { describe, expect, it } from "vitest";

import { torrentsLink } from "./torrentsSearch";

describe("the Torrents link", () => {
  it("opens the search at a title, a season and an episode", () => {
    expect(torrentsLink({ title: "show:95396", season: 2, episode: 5 })).toBe(
      "/torrents?title=show%3A95396&season=2&episode=5",
    );
    expect(torrentsLink({ title: "movie:693134" })).toBe("/torrents?title=movie%3A693134");
  });
});
