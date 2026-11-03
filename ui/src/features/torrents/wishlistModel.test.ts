import { describe, expect, it } from "vitest";

import { createFormatters } from "../../i18n/format";
import type { LibraryCopy } from "./ownedCopy";
import type { WishCriteria, WishEntry } from "./torrentBackend";
import {
  criteriaText,
  defaultCriteria,
  defaultScope,
  draftOf,
  draftProblem,
  draftValue,
  orderEntries,
  scopeText,
  sizeUnitBytes,
} from "./wishlistModel";

const format = createFormatters("en-GB", "base10");
const GB = sizeUnitBytes("base10");

function copy(resolution: string, hdr: string | null): LibraryCopy {
  return {
    itemId: "1",
    resolution,
    source: "BluRay",
    hdr,
    codec: null,
    audio: null,
    edition: null,
    languages: [],
    size: 1,
    fileName: "x.mkv",
  };
}

const ANY: WishCriteria = {
  minResolution: null,
  sources: [],
  hdr: false,
  maxSize: null,
  minSeeders: null,
};

describe("the Wishlist model", () => {
  it("starts one step above the copy held", () => {
    expect(defaultCriteria(null).minResolution).toBe("1080p");
    expect(defaultCriteria(copy("720p", null)).minResolution).toBe("1080p");
    expect(defaultCriteria(copy("1080p", null)).minResolution).toBe("2160p");
    expect(defaultCriteria(copy("2160p", null))).toMatchObject({
      minResolution: "2160p",
      hdr: true,
      sources: [],
    });
    expect(defaultCriteria(copy("2160p", "DV"))).toMatchObject({ hdr: true, sources: ["Remux"] });
  });

  it("starts a show from the season or episode asked for, else the next new episode", () => {
    expect(defaultScope("movie", 2, 3).kind).toBe("movie");
    expect(defaultScope("show", undefined, undefined).kind).toBe("next");
    expect(defaultScope("show", 2, undefined)).toEqual({
      kind: "season",
      season: 2,
      episode: null,
    });
    expect(defaultScope("show", 2, 5)).toEqual({ kind: "episode", season: 2, episode: 5 });
  });

  it("says the criteria in one line", () => {
    expect(criteriaText(ANY, format)).toBe("Any resolution");
    expect(
      criteriaText(
        {
          minResolution: "2160p",
          sources: ["Remux", "BluRay"],
          hdr: true,
          maxSize: 80 * GB,
          minSeeders: 10,
        },
        format,
      ),
    ).toMatch(/^2160p or better · Remux, Blu-ray · HDR · up to 80\sGB · 10\+ seeders$/);
  });

  it("reads the editor's fields back and refuses bad numbers", () => {
    const criteria: WishCriteria = { ...ANY, minResolution: "720p", maxSize: 40 * GB };
    const draft = draftOf({ kind: "season", season: 3, episode: null }, criteria, GB);
    expect(draft.maxSizeGb).toBe("40");
    expect(draftValue(draft, GB)).toEqual({
      scope: { kind: "season", season: 3, episode: null },
      criteria,
    });
    expect(draftProblem({ ...draft, maxSizeGb: "lots" })).not.toBeNull();
    expect(draftProblem({ ...draft, minSeeders: "2.5" })).not.toBeNull();
    expect(draftProblem({ ...draft, maxSizeGb: "12,5", minSeeders: "" })).toBeNull();
    expect(draftValue({ ...draft, maxSizeGb: "12,5" }, GB).criteria.maxSize).toBe(12.5 * GB);
  });

  it("names the scope and lists the newest entries first, done ones last", () => {
    const entry = (id: number, added: number, done: number | null): WishEntry => ({
      id,
      kind: "show",
      tmdbId: id,
      title: `Show ${id}`,
      year: 2020,
      posterKey: null,
      scope: { kind: "next", season: null, episode: null },
      criteria: ANY,
      added,
      checked: null,
      note: null,
      noteDetail: null,
      done,
      target: { season: 3, episode: 6 },
      inLibrary: null,
      matches: [],
    });
    expect(scopeText(entry(1, 0, null))).toBe("Next new episode · S03E06");
    const ordered = orderEntries([entry(1, 10, null), entry(2, 30, 5), entry(3, 20, null)]);
    expect(ordered.active.map((e) => e.id)).toEqual([3, 1]);
    expect(ordered.done.map((e) => e.id)).toEqual([2]);
  });
});
