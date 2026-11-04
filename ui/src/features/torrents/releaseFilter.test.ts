import { describe, expect, it } from "vitest";

import { isEmptyFilter, matchesFilter, parseReleaseFilter } from "./releaseFilter";

const NAME = "Example.Movie.2020.2160p.UHD.BluRay.REMUX.HDR.TrueHD.Atmos.7.1-GRP";

const matches = (text: string, name = NAME) => matchesFilter(name, parseReleaseFilter(text));

describe("release filter", () => {
  it("needs every word, in any order and case", () => {
    expect(matches("atmos remux")).toBe(true);
    expect(matches("REMUX Atmos 2160P")).toBe(true);
    expect(matches("atmos x265")).toBe(false);
  });

  it("leaves out names holding a word that starts with a dash", () => {
    expect(matches("2160p -x265")).toBe(true);
    expect(matches("-remux")).toBe(false);
    expect(matches("1080p -x265", "Example.Movie.2020.1080p.WEB-DL.x265-GRP")).toBe(false);
  });

  it("ignores punctuation on both sides", () => {
    for (const name of ["A.1080p.WEB-DL", "A.1080p.WEB.DL", "A 1080p WEBDL"]) {
      expect(matches("web-dl", name)).toBe(true);
    }
    expect(matches("7.1")).toBe(true);
    expect(matches("true-hd")).toBe(true);
  });

  it("ignores a lone dash and blank text", () => {
    expect(isEmptyFilter(parseReleaseFilter("  -  "))).toBe(true);
    expect(isEmptyFilter(parseReleaseFilter(""))).toBe(true);
    expect(matches(" - ")).toBe(true);
  });
});
