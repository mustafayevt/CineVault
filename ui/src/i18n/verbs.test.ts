import { describe, expect, it } from "vitest";

import { allMessages } from "./catalog";

/**
 * The retired verbs (spec `ui-design-system` "One verb glossary"): the UI says Scan, Scrape and
 * Rename, and a title's stage is New, Scraped or Renamed. Scraping a scraped title opens the
 * match picker; there is no "Rescrape", "Scrape again" or "Change match".
 */
const RETIRED =
  /\b(identif(?:y|ies|ied|ying)|enrich\w*|rescrap\w*|scrape again|change match|organi[sz](?:e|es|ed|ing)|search & scrape)\b/i;

describe("the string catalogue", () => {
  it("uses no retired verb in any message", () => {
    const hits = Object.entries(allMessages())
      .filter(([, text]) => RETIRED.test(text))
      .map(([key, text]) => `${key}: ${text}`);
    expect(hits).toEqual([]);
  });

  it("recognises each retired word", () => {
    for (const word of [
      "Identify",
      "Enrich",
      "Rescrape",
      "Scrape again",
      "Change match",
      "Organize",
      "Organized",
      "Search & scrape",
    ]) {
      expect(RETIRED.test(`Click ${word} now`), word).toBe(true);
    }
    expect(RETIRED.test("Scrape")).toBe(false);
    expect(RETIRED.test("Rename 3 titles")).toBe(false);
  });
});
