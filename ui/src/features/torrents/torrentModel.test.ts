import { describe, expect, it } from "vitest";

import type { ProviderItem } from "../workspaces/providerEvents";
import { syntheticReleases } from "./syntheticTorrents";
import { release } from "./torrentHarness";
import type { TitleHit } from "./torrentBackend";
import {
  ageText,
  mergeReleases,
  mergeTitleHits,
  noSourceAnswered,
  seedHealth,
  siteName,
  titleName,
} from "./torrentModel";

const from = <T>(items: T[], provider = "p"): ProviderItem<T>[] =>
  items.map((item) => ({ provider, item }));

function hit(overrides: Partial<TitleHit>): TitleHit {
  return {
    id: "movie:1",
    kind: "movie",
    tmdbId: 1,
    title: "Dune",
    originalTitle: null,
    year: 2021,
    overview: null,
    posterKey: null,
    imdbId: null,
    confidence: 0.5,
    inLibrary: null,
    ...overrides,
  };
}

describe("title hits", () => {
  it("replaces a hit sent again and puts the best match first", () => {
    const merged = mergeTitleHits(
      from([
        hit({ id: "movie:1", confidence: 0.4 }),
        hit({ id: "show:2", kind: "show", confidence: 0.9 }),
        hit({ id: "movie:1", confidence: 0.4, posterKey: "k" }),
      ]),
    );
    expect(merged.map((h) => h.id)).toEqual(["show:2", "movie:1"]);
    expect(merged[1]?.posterKey).toBe("k");
  });

  it("keeps the arrival order of equal hits and treats a missing confidence as zero", () => {
    const merged = mergeTitleHits(
      from([
        hit({ id: "movie:1", confidence: 0.5 }),
        hit({ id: "movie:2", confidence: 0.5 }),
        hit({ id: "movie:3", confidence: null }),
      ]),
    );
    expect(merged.map((h) => h.id)).toEqual(["movie:1", "movie:2", "movie:3"]);
  });

  it("names a title with its year when it has one", () => {
    expect(titleName({ title: "Dune", year: 2021 })).toBe("Dune (2021)");
    expect(titleName({ title: "Dune", year: null })).toBe("Dune");
    expect(titleName({ title: "Dune", year: 0 })).toBe("Dune");
  });
});

describe("merging releases", () => {
  it("replaces a row sent again and drops the rows it absorbs", () => {
    const merged = mergeReleases(
      from([
        release({ id: "n:1", name: "Same", sources: ["a"] }),
        release({ id: "h1", name: "Other" }),
        release({ id: "h2", name: "Same", sources: ["a", "b"], absorbs: ["n:1"] }),
        release({ id: "h1", name: "Other", seeders: 99 }),
      ]),
    );
    expect(merged.map((r) => r.id)).toEqual(["h1", "h2"]);
    expect(merged[0]?.seeders).toBe(99);
    expect(merged[1]?.sources).toEqual(["a", "b"]);
  });
});

describe("rows", () => {
  it("rates the seeders in three steps", () => {
    expect([null, 0, 3, 19, 20, 99, 100, 5000].map(seedHealth)).toEqual([0, 0, 1, 1, 2, 2, 3, 3]);
  });

  it("names a site without the indexer mark", () => {
    expect(siteName("1337x (indexer)")).toBe("1337x");
    expect(siteName("YTS")).toBe("YTS");
  });
});

describe("age", () => {
  const now = Date.UTC(2026, 9, 26);
  const day = 86_400_000;
  it("is short", () => {
    expect(ageText(now - 3600_000, now)).toBe("today");
    expect(ageText(now - 3 * day, now)).toBe("3 d");
    expect(ageText(now - 90 * day, now)).toBe("3 mo");
    expect(ageText(now - 800 * day, now)).toBe("2 y");
    expect(ageText(null, now)).toBe("");
  });
});

describe("source words", () => {
  it("knows when no source answered", () => {
    const failed = { provider: "a", state: "failed" as const, count: 0, retryable: true };
    const done = { provider: "b", state: "done" as const, count: 0, retryable: false };
    expect(noSourceAnswered([failed, { ...failed, provider: "c" }], false)).toBe(true);
    expect(noSourceAnswered([failed, done], false)).toBe(false);
    expect(noSourceAnswered([failed], true)).toBe(false);
    expect(noSourceAnswered([], false)).toBe(false);
  });
});

describe("the synthetic releases", () => {
  it("are the redesign's rows with unique ids and each source's page", () => {
    const rows = syntheticReleases({ kind: "movie", tmdbId: 1 });
    expect(rows).toHaveLength(12);
    expect(new Set(rows.map((r) => r.id)).size).toBe(12);
    expect(new Set(rows.map((r) => r.resolution))).toEqual(new Set(["2160p", "1080p", "720p"]));
    const first = rows[0];
    expect(first?.pages.map((page) => page.source)).toEqual(["apibay", "knaben", "idx-1337x"]);
    expect(rows.filter((r) => !r.matching)).toHaveLength(2);
  });

  it("are marked against the library copy only for a title in the library", () => {
    expect(syntheticReleases({ kind: "movie", tmdbId: 1 }).some((r) => r.upgrade !== null)).toBe(
      true,
    );
    expect(syntheticReleases({ kind: "movie", tmdbId: 2 }).every((r) => r.upgrade === null)).toBe(
      true,
    );
  });

  it("name episodes and packs for a show", () => {
    const episode = syntheticReleases({ kind: "show", tmdbId: 1, season: 2, episode: 4 });
    expect(episode.filter((r) => r.pack === null).every((r) => r.name.includes("S02E04"))).toBe(
      true,
    );
    expect(episode.filter((r) => r.pack !== null)).toHaveLength(3);
    expect(syntheticReleases({ kind: "show", tmdbId: 1, season: 2 })[0]?.pack).toBe("season");
    expect(syntheticReleases({ kind: "show", tmdbId: 1 })[0]?.pack).toBe("complete");
    expect(syntheticReleases({ kind: "show", tmdbId: 99 })).toEqual([]);
    expect(syntheticReleases({ kind: "movie", tmdbId: 92 })).toEqual([]);
  });
});
