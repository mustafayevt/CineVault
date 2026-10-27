import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ProviderEvent } from "../workspaces/providerEvents";
import type { ScrapeFieldDiff, ScrapeStreamHit } from "./scrapeBackend";
import { createSyntheticScrapePreview, createSyntheticScrapeStream } from "./syntheticScrape";

function run(query: string) {
  const fake = createSyntheticScrapeStream(query);
  const events: ProviderEvent<ScrapeStreamHit>[] = [];
  fake.transport.start(
    { searchId: "s1", query, params: { itemId: "1", kind: "movie", language: "en" } },
    (event) => events.push(event),
  );
  return events;
}

describe("the synthetic scrape stream", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("sends an entry again under the same id when a second provider knows the title", async () => {
    const events = run("solaris");
    await vi.advanceTimersByTimeAsync(400);

    const results = events.filter((event) => event.kind === "results");
    expect(results.map((event) => ("provider" in event ? event.provider : ""))).toEqual([
      "omdb",
      "tmdb",
    ]);
    const [early, late] = results.map((event) => (event.kind === "results" ? event.items : []));
    expect(early?.map((hit) => hit.id)).toEqual(late?.map((hit) => hit.id));
    expect(early?.[0]?.providers).toHaveLength(1);
    expect(late?.[0]?.providers).toHaveLength(2);
    expect(events.some((event) => event.kind === "needsKey")).toBe(true);
    expect(events.at(-1)).toEqual({ kind: "done" });
  });

  it("says why nothing was found just before done", async () => {
    const events = run("zzyzx");
    await vi.advanceTimersByTimeAsync(400);

    const last = events.slice(-2);
    expect(last[0]).toMatchObject({ kind: "failed", provider: "search", retryable: false });
    expect(last[1]).toEqual({ kind: "done" });
  });
});

describe("the synthetic scrape preview", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the search result's fields first and every field when the details arrive", async () => {
    const fake = createSyntheticScrapePreview();
    const events: ProviderEvent<ScrapeFieldDiff>[] = [];
    fake.transport.start({ searchId: "p1", query: "" }, (event) => events.push(event));

    await vi.advanceTimersByTimeAsync(10);
    const early = events.find((event) => event.kind === "results");
    expect(early?.kind === "results" ? early.items.map((row) => row.field) : []).toEqual([
      "title",
      "year",
      "runtime",
    ]);
    expect(events.some((event) => event.kind === "done")).toBe(false);

    await vi.advanceTimersByTimeAsync(400);
    const results = events.filter((event) => event.kind === "results");
    const late = results[1];
    const fields = late?.kind === "results" ? late.items : [];
    expect(fields.map((row) => row.group)).toEqual(
      expect.arrayContaining(["text", "people", "ratings-and-ids", "artwork"]),
    );
    expect(fields.find((row) => row.field === "plot")?.locked).toBe(true);
    expect(events.at(-1)).toEqual({ kind: "done" });
  });
});
