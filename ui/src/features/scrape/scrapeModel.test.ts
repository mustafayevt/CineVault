import { describe, expect, it } from "vitest";

import type { ProviderItem } from "../workspaces/providerEvents";
import type { ScrapeStreamHit } from "./scrapeBackend";
import { mergeScrapeHits } from "./scrapeModel";

function hit(id: string, year: number, confidence: number, rank: number): ScrapeStreamHit {
  return {
    id,
    candidate: {
      provider: "tmdb",
      providerId: id,
      title: "Memento",
      originalTitle: null,
      year,
      director: null,
      runtimeMinutes: null,
      language: null,
      overview: null,
      posterKey: null,
      confidence,
      rank,
      evidence: [],
      breakdown: { similarity: 1, yearPenalty: 0, posterPenalty: 0, evidence: 0 },
    },
    providers: [{ provider: "tmdb", providerId: id, confidence }],
    ids: [],
    inLibrary: null,
    absorbs: [],
  };
}

const item = (value: ScrapeStreamHit): ProviderItem<ScrapeStreamHit> => ({
  provider: "tmdb",
  item: value,
});

describe("mergeScrapeHits", () => {
  it("lists by rank, so of two equal confidences the better known title leads (QA-2)", () => {
    // The obscure 2025 short arrives first and shows the same 74% as the 2000 film.
    const merged = mergeScrapeHits([
      item(hit("a", 2025, 0.74, 0.74 + 0.04)),
      item(hit("b", 2000, 0.74, 0.74 + 0.1)),
    ]);
    expect(merged.map((entry) => entry.candidate.year)).toEqual([2000, 2025]);
  });

  it("keeps the arrival order of equal ranks", () => {
    const merged = mergeScrapeHits([
      item(hit("a", 2001, 0.9, 0.9)),
      item(hit("b", 2002, 0.9, 0.9)),
    ]);
    expect(merged.map((entry) => entry.id)).toEqual(["a", "b"]);
  });
});
