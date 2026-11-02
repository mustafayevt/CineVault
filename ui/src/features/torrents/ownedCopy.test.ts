import { describe, expect, it } from "vitest";

import {
  audioRank,
  editionOf,
  missingText,
  reasonOf,
  seasonsText,
  showHoldings,
  showMarkOf,
  verdictSentence,
} from "./ownedCopy";
import type { LibraryCopy } from "./ownedCopy";
import { release } from "./torrentHarness";

const copy = (overrides: Partial<LibraryCopy> = {}): LibraryCopy => ({
  itemId: "1",
  resolution: "1080p",
  source: "WEB-DL",
  hdr: null,
  codec: "AVC",
  audio: "E-AC-3 5.1",
  edition: null,
  languages: [],
  size: 6_000_000_000,
  fileName: "x.mkv",
  ...overrides,
});

const span = (
  firstSeason: number | null,
  lastSeason: number | null,
  firstEpisode: number | null = null,
  lastEpisode: number | null = firstEpisode,
) => ({ firstSeason, lastSeason, firstEpisode, lastEpisode });

describe("the reason of a tag", () => {
  it("names both qualities, and the source when the picture is the same", () => {
    expect(reasonOf("better", copy(), release({ resolution: "2160p", hdr: "DV" }))).toBe(
      "Better: 2160p Dolby Vision vs your 1080p SDR",
    );
    expect(
      reasonOf("better", copy(), release({ resolution: "1080p", hdr: null, source: "Remux" })),
    ).toBe("Better: 1080p SDR Remux vs your 1080p SDR WEB-DL");
  });
});

describe("the verdict sentence", () => {
  it("reads the backend verdict, the audio and the size", () => {
    const theirs = release({ audio: "TrueHD Atmos 7.1", size: 13_800_000_000 });
    expect(verdictSentence("better", copy(), theirs)).toBe(
      "Better picture, better audio, 2.3× the size",
    );
    expect(verdictSentence("same", copy(), release({ audio: "DD+ 5.1", size: null }))).toBe(
      "Same picture, same audio",
    );
    expect(verdictSentence("worse", copy({ audio: null }), release({ size: 3_000_000_000 }))).toBe(
      "Worse picture, 0.5× the size",
    );
  });

  it("ranks audio formats by their names", () => {
    expect(audioRank("TrueHD Atmos 7.1")).toBeGreaterThan(audioRank("DTS-HD MA 5.1"));
    expect(audioRank("DTS-HD MA 5.1")).toBeGreaterThan(audioRank("DTS 5.1"));
    expect(audioRank("E-AC-3 5.1")).toBe(audioRank("DD+ 5.1"));
    expect(audioRank("AC3")).toBeLessThan(audioRank("DD+"));
    expect(audioRank(null)).toBe(0);
  });

  it("reads an edition from a release name", () => {
    expect(editionOf("Heat.1995.Directors.Cut.1080p")).toBe("Director's Cut");
    expect(editionOf("Aliens.1986.EXTENDED.2160p")).toBe("Extended");
    expect(editionOf("Heat.1995.1080p")).toBeNull();
  });
});

describe("show releases against the episodes held", () => {
  const holdings = showHoldings(
    [
      { season: 1, episode: 1 },
      { season: 1, episode: 2 },
      { season: 2, episode: 1 },
    ],
    [
      { season: 2, episode: 2 },
      { season: 2, episode: 3 },
    ],
  );

  it("marks a release with a missing episode as filling a gap", () => {
    expect(showMarkOf(release({ episodes: span(2, 2) }), holdings)).toBe("gap");
    expect(showMarkOf(release({ episodes: span(2, 2, 3) }), holdings)).toBe("gap");
    expect(showMarkOf(release({ episodes: span(null, null) }), holdings)).toBe("gap");
  });

  it("marks a release of held episodes as in the library, or better", () => {
    expect(showMarkOf(release({ episodes: span(1, 1), upgrade: "worse" }), holdings)).toBe("held");
    expect(showMarkOf(release({ episodes: span(1, 1, 2), upgrade: "better" }), holdings)).toBe(
      "better",
    );
  });

  it("leaves a release of seasons the library knows nothing of untagged", () => {
    expect(showMarkOf(release({ episodes: span(7, 7) }), holdings)).toBeNull();
    expect(showMarkOf(release({ episodes: null }), holdings)).toBeNull();
  });

  it("says the seasons held and the missing episodes as runs", () => {
    const held = [1, 2, 3, 5].map((season) => ({ season, episode: 1 }));
    expect(seasonsText(held)).toBe("S01–S03, S05");
    const missing = [5, 6, 7, 8].map((episode) => ({ season: 4, episode }));
    expect(missingText([...missing, { season: 5, episode: 1 }])).toBe("S04E05–E08, S05E01");
    const many = [1, 3, 5, 7, 9].map((episode) => ({ season: 1, episode }));
    expect(missingText(many)).toBe("S01E01, S01E03, S01E05 and 2 more");
  });
});
