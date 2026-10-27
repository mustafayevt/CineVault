import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ProviderEvent } from "../workspaces/providerEvents";
import type { ArtworkCandidate } from "./artworkBackend";
import { createSyntheticArtworkStream, syntheticCurrentArtwork } from "./syntheticArtwork";

function collect(slot: string) {
  const fake = createSyntheticArtworkStream(slot);
  const events: ProviderEvent<ArtworkCandidate>[] = [];
  fake.transport.start({ searchId: "s1", query: "", params: { itemId: "1", slot } }, (event) =>
    events.push(event),
  );
  return { fake, events };
}

describe("the synthetic artwork stream", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("lets the quick provider answer first, fails TheTVDB once and delivers it on retry", async () => {
    const { fake, events } = collect("poster");
    await vi.advanceTimersByTimeAsync(650);

    const answers = events.filter((event) => event.kind !== "started").map((event) => event.kind);
    expect(events.slice(0, 3).map((event) => event.kind)).toEqual([
      "started",
      "started",
      "started",
    ]);
    expect(answers[0]).toBe("results");
    expect(events.find((event) => event.kind === "failed")).toMatchObject({
      provider: "tvdb",
      retryable: true,
    });

    events.length = 0;
    fake.transport.retry("s1", "tvdb");
    await vi.advanceTimersByTimeAsync(150);
    expect(events.map((event) => event.kind)).toEqual(["started", "results", "done"]);
  });

  it("hands the slow provider's images over in batches, the last one ending its answer", async () => {
    const { events } = collect("poster");
    await vi.advanceTimersByTimeAsync(700);
    const tmdb = events.filter(
      (event) => "provider" in event && event.provider === "tmdb" && event.kind !== "started",
    );
    expect(tmdb.map((event) => event.kind)).toEqual(["partial", "results"]);
    const sizes = tmdb.map((event) => ("items" in event ? event.items.length : 0));
    expect(sizes).toEqual([3, 3]);
  });

  it("makes eight frames last for backdrops and none for posters", async () => {
    const backdrop = collect("fanart");
    await vi.advanceTimersByTimeAsync(700);
    const frames = backdrop.events.find(
      (event) => event.kind === "results" && event.provider === "frames",
    );
    expect(frames?.kind === "results" ? frames.items : []).toHaveLength(8);
    const last = backdrop.events.filter((event) => event.kind === "results").at(-1);
    expect(last).toMatchObject({ provider: "frames" });

    const poster = collect("poster");
    await vi.advanceTimersByTimeAsync(700);
    expect(poster.events.some((event) => "provider" in event && event.provider === "frames")).toBe(
      false,
    );
  });

  it("leaves out the providers that do not offer the slot and pins the current image", () => {
    const { events } = collect("disc");
    const started = events.filter((event) => event.kind === "started");
    expect(started.map((event) => ("provider" in event ? event.provider : ""))).toEqual(["fanart"]);
    expect(syntheticCurrentArtwork("poster").provider).toBe("current");
  });
});
