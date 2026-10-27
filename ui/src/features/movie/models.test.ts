import { describe, expect, it } from "vitest";

import type { MovieDetail } from "../detail/detailBackend";
import { addedAgo, initials, languageName, metaLine, runtimeLabel } from "../detail/detailFormat";
import { heroAudioWhy, sameLanguage } from "../detail/heroAudio";
import { syntheticMovieDetail } from "../detail/syntheticDetail";
import { createFormatters } from "../../i18n/format";
import {
  durationText,
  hdrFacts,
  hdrLabel,
  overallBitrate,
  techSummary,
  totalVideoSize,
} from "./techModel";

const format = createFormatters("en-US", "base10");

describe("the detail formatters", () => {
  it("writes runtimes, meta lines and relative dates", () => {
    expect(runtimeLabel(166)).toBe("2h 46m");
    expect(runtimeLabel(120)).toBe("2h");
    expect(runtimeLabel(46)).toBe("46 min");
    expect(runtimeLabel(0)).toBe("");
    expect(metaLine(syntheticMovieDetail())).toBe(
      "2024 · PG-13 · 2h 46m · Science Fiction, Adventure",
    );
    const now = Date.UTC(2026, 9, 20);
    expect(addedAgo(now - 3 * 86_400_000, now, "en")).toBe("3 days ago");
    expect(addedAgo(now, now, "en")).toBe("today");
    expect(addedAgo(null, now, "en")).toBe("");
  });

  it("names languages and takes initials", () => {
    expect(languageName("tr", "en")).toBe("Turkish");
    expect(languageName("Klingon dub", "en")).toBe("Klingon dub");
    expect(initials("Timothée Chalamet")).toBe("TC");
  });
});

describe("the hero audio rules", () => {
  it("says why for each rule: original language, default and first track", () => {
    const base = syntheticMovieDetail().hero_audio as NonNullable<MovieDetail["hero_audio"]>;
    const why = (flags: string[], language: string, original: string) =>
      heroAudioWhy({ ...base, flags, language }, original, "en-US");
    expect(why(["original"], "eng", "en")).toMatch(/marked as the original version/);
    // `en` and `eng` are the same language.
    expect(why([], "eng", "en")).toMatch(/first one in the original language \(English\)/);
    expect(why(["default"], "fra", "en")).toMatch(/is the default track/);
    expect(why([], "fra", "en")).toMatch(/is the first track/);
    expect(sameLanguage("en", "eng")).toBe(true);
    expect(sameLanguage("en", "fra")).toBe(false);
    expect(sameLanguage("", "en")).toBe(false);
  });
});

describe("the Technical summary", () => {
  it("reads the video headline, the audio and subtitle rows and the file facts", () => {
    const detail = syntheticMovieDetail();
    const summary = techSummary(detail, format);
    expect(summary.videoHeadline).toBe("2160p · Dolby Vision Profile 8.1 · HDR10");
    expect(summary.audio.length).toBeGreaterThan(0);
    expect(summary.subtitles.length).toBe(detail.subtitles.length);
    expect(summary.fileName).toBe(detail.videos[0]?.name);
    expect(summary.fileCount).toBe(detail.videos.length + detail.files.length);
  });

  it("names the HDR formats precisely and lists the mastering display and light levels", () => {
    const stream = syntheticMovieDetail().videos[0]?.streams[0] as NonNullable<
      NonNullable<MovieDetail["videos"][number]>["streams"][number]
    >;
    expect(hdrLabel(stream)).toBe("Dolby Vision Profile 8.1 · HDR10");
    expect(hdrFacts(stream, format)).toEqual([
      { label: "Dolby Vision", value: "Profile 8.1 · Base layer HDR10" },
      { label: "Mastering display", value: "Display P3 · 0.005 to 1,000 cd/m²" },
      { label: "MaxCLL", value: "1,000 cd/m²" },
      { label: "MaxFALL", value: "400 cd/m²" },
    ]);
    // Read before the details were kept: the format names alone, and no facts.
    const old = { ...stream, hdr: ["HDR10+"], hdr_details: null };
    expect(hdrLabel(old)).toBe("HDR10+");
    expect(hdrFacts(old, format)).toEqual([]);
    expect(hdrLabel({ hdr: ["Dolby Vision"], hdr_details: null })).toBe("Dolby Vision");
    expect(hdrLabel({ hdr: ["HLG"], hdr_details: null })).toBe("HLG");
    expect(hdrLabel({ hdr: [], hdr_details: null })).toBe("");
  });

  it("sums only the files that have a size, and says nothing without any", () => {
    expect(totalVideoSize([])).toBeNull();
    const [video] = syntheticMovieDetail().videos;
    const sized = { ...(video as NonNullable<typeof video>), size_bytes: 1000 };
    const unsized = { ...sized, size_bytes: null };
    expect(totalVideoSize([sized, unsized, sized])).toBe(2000);
    expect(totalVideoSize([unsized])).toBeNull();
  });

  it("derives the overall bitrate and the duration text", () => {
    expect(overallBitrate(1_000_000_000, 1000, format)).toBe("8 Mb/s");
    expect(overallBitrate(null, 1000, format)).toBe("");
    expect(overallBitrate(1000, 0, format)).toBe("");
    expect(durationText(9972)).toBe("2h 46m 12s");
    expect(durationText(65)).toBe("1m 5s");
    expect(durationText(0)).toBe("");
  });
});
