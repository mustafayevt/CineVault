import { describe, expect, it } from "vitest";
import {
  FALLBACK_ORDER,
  formatRatingValue,
  pickRating,
  ratingOrder,
  ratingSource,
  toNativeScale,
} from "./ratingSources";
import { normalisePreferredSource } from "./preference";
import { createFormatters } from "../../i18n/format";
import { SETTINGS, settingDef } from "../settings/schema";

const format = createFormatters("en-US", "base10");
const digits = (n: number) => format.number(n);

describe("rating values are written the way each source writes them", () => {
  it("keeps Letterboxd on its five-point scale", () => {
    expect(formatRatingValue("letterboxd", 4.4, 5, digits)).toBe("4.4");
    expect(toNativeScale("letterboxd", 8.8)).toBeCloseTo(4.4);
    expect(ratingSource("letterboxd")?.max).toBe(5);
  });

  it("writes Rotten Tomatoes as a percentage and Metacritic as a score", () => {
    expect(formatRatingValue("tomatometerallcritics", 92, 100, digits)).toBe("92%");
    expect(formatRatingValue("metacritic", 79, 100, digits)).toBe("79");
    expect(formatRatingValue("imdb", 8.46, 10, digits)).toBe("8.5");
    // An unknown source on a percentage scale is written as one.
    expect(formatRatingValue("somewhere", 85, 100, digits)).toBe("85%");
  });

  it("converts the ten-point scale of the library rows to a source's own scale", () => {
    expect(toNativeScale("tomatometerallcritics", 9.2)).toBeCloseTo(92);
    expect(toNativeScale("rogerebert", 8.75)).toBeCloseTo(3.5);
    expect(toNativeScale("imdb", 8.5)).toBe(8.5);
  });
});

describe("the preferred rating and its fallback", () => {
  const ratings = [
    { source: "imdb", value: 8.3, max: 10 },
    { source: "tmdb", value: 7.9, max: 10 },
    { source: "user", value: 10, max: 10 },
  ];

  it("shows the preferred source's rating, not marked as a fallback", () => {
    const picked = pickRating(ratings, "tmdb");
    expect(picked?.rating.source).toBe("tmdb");
    expect(picked?.fallback).toBe(false);
  });

  it("falls back to the next available source and says so", () => {
    const withoutImdb = ratings.filter((r) => r.source !== "imdb");
    const picked = pickRating(withoutImdb, "imdb");
    expect(picked?.rating.source).toBe("tmdb");
    expect(picked?.fallback).toBe(true);
  });

  it("never offers the user's own rating, and has nothing without a source rating", () => {
    expect(pickRating([{ source: "user", value: 9, max: 10 }], "imdb")).toBeNull();
    expect(pickRating([], "imdb")).toBeNull();
  });

  it("tries the preferred source first, then the fallback order without repeating it", () => {
    const order = ratingOrder("letterboxd");
    expect(order[0]).toBe("letterboxd");
    expect(order.slice(1)).toEqual(FALLBACK_ORDER.filter((source) => source !== "letterboxd"));
  });

  it("takes only a source the app knows as the preference", () => {
    expect(normalisePreferredSource("IMDb")).toBe("imdb");
    expect(normalisePreferredSource("nonsense")).toBe("tmdb");
    expect(normalisePreferredSource(3)).toBe("tmdb");
    expect(normalisePreferredSource(undefined)).toBe("tmdb");
  });
});

describe("the rating settings", () => {
  it("prefers TMDB until a person chooses another source, and lists the sources by name", () => {
    const preferred = settingDef("ratings.preferred-source");
    expect(preferred?.default).toBe("tmdb");
    expect(preferred?.control.type).toBe("choice");
    const names =
      preferred?.control.type === "choice" ? preferred.control.options.map((o) => o.label) : [];
    expect(names).toEqual(
      expect.arrayContaining([
        "IMDb",
        "TMDB",
        "Letterboxd",
        "Rotten Tomatoes critics",
        "Metacritic",
      ]),
    );
  });

  it("matches the defaults the Rust side reads for fetching ratings", () => {
    // cv_app::ratings::rating_settings_from_values: movies and shows both fetch after a scrape.
    // RatingFetchSettings::DEFAULT_SOURCES: a keyed OMDb or MDBList is used without choosing it.
    expect(settingDef("ratings.sources-movie")?.default).toBe("imdb-dataset,omdb,mdblist");
    expect(settingDef("ratings.sources-show")?.default).toBe("imdb-dataset,omdb,mdblist");
    expect(settingDef("ratings.fetch-after-scrape-movie")?.default).toBe(true);
    expect(settingDef("ratings.fetch-after-scrape-show")?.default).toBe(true);
    const checklist = settingDef("ratings.sources-movie")?.control;
    expect(checklist?.type === "checklist" ? checklist.options.map((o) => o.value) : []).toEqual([
      "imdb-dataset",
      "scraper",
      "omdb",
      "mdblist",
    ]);
  });

  it("gives every rating setting a title, a description and search words", () => {
    for (const def of SETTINGS.filter((setting) => setting.id.startsWith("ratings."))) {
      expect(def.title, def.id).not.toBe("");
      expect(def.description, def.id).not.toBe("");
      expect(def.keywords.length, def.id).toBeGreaterThan(0);
    }
  });
});
