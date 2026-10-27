import { describe, expect, it } from "vitest";

import { createSyntheticBackend, foldForSearch, syntheticRow } from "./syntheticLibrary";
import type { LibraryQuery } from "./libraryBackend";

describe("the quick search of the sample library", () => {
  it("folds punctuation, spacing, diacritics and apostrophes", () => {
    expect(foldForSearch("Spider-Man")).toBe("spider man");
    expect(foldForSearch("WALL·E")).toBe("wall e");
    expect(foldForSearch("Amélie")).toBe("amelie");
    expect(foldForSearch("Schindler's List")).toBe("schindlers list");
    expect(foldForSearch("Pride & Prejudice")).toBe("pride and prejudice");
    expect(foldForSearch("  ...  ")).toBe("");
  });

  it("finds a title however the words are spelled", async () => {
    const backend = createSyntheticBackend(40);
    const title = syntheticRow(3).card.title;
    const query = (search: string): LibraryQuery => ({
      kind: "movies",
      search,
      sort: [],
      filters: [],
    });
    const squashed = foldForSearch(title).replaceAll(" ", "");
    for (const search of [title, title.toUpperCase(), squashed]) {
      const page = await backend.window(query(search), 0, 40);
      expect(page.items.map((card) => card.title)).toContain(title);
    }
    expect((await backend.window(query("zzzz qqqq"), 0, 40)).total).toBe(0);
  });
});
