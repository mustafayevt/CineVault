import { describe, expect, it } from "vitest";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { syntheticCard } from "../library/syntheticLibrary";
import { createSyntheticLifecycleBackend } from "./lifecycleBackend";

describe("the fixture lifecycle", () => {
  it("gives every card a lifecycle that follows the backend rules", () => {
    for (let index = 0; index < 600; index += 1) {
      const card = syntheticCard(index);
      const lifecycle = card.lifecycle;
      expect(lifecycle).not.toBeNull();
      if (lifecycle === null) continue;
      const kinds = lifecycle.flags.map((flag) => flag.kind);
      // Locked outranks Rename pending: a title never carries both.
      expect(kinds.includes("locked") && kinds.includes("rename-pending")).toBe(false);
      // An unmatched title is New with the Not matched flag and its reason.
      if (card.unmatched) {
        expect(lifecycle.stage).toBe("new");
        expect(lifecycle.flags[0]).toEqual({ kind: "not-matched", reason: "No candidates found" });
      }
      // A title that needs review stays New.
      if (card.needs_review && !card.unmatched) {
        expect(lifecycle.stage).toBe("new");
        expect(kinds).toEqual(["needs-review"]);
      }
      expect(lifecycle.lines.map((line) => line.step)).toEqual(["scan", "scrape", "rename"]);
    }
  });

  it("counts Needs review as uncertain plus not matched, and the sidebar agrees", async () => {
    const library = createSyntheticBackend(181);
    const lifecycle = createSyntheticLifecycleBackend(181);
    const [movies, shows, counts] = await Promise.all([
      lifecycle.counts("movies"),
      lifecycle.counts("shows"),
      library.counts(),
    ]);
    expect(movies.total).toBe(181);
    expect(movies.new + movies.scraped + movies.renamed).toBe(movies.total);
    expect(movies.needs_review).toBe(movies.uncertain + movies.not_matched);
    expect(movies.not_matched_top_reason?.count).toBe(movies.not_matched);
    // The sidebar badge, Home and the Needs review page read the same number.
    expect(counts.needs_review).toBe(movies.needs_review + shows.needs_review);
  });

  it("answers per item and leaves out an id it does not know", async () => {
    const backend = createSyntheticLifecycleBackend(50);
    const answer = await backend.get(["1", "2", "nope"]);
    expect(answer.map((entry) => entry.id)).toEqual(["1", "2"]);
    expect(answer[0]?.lifecycle).toEqual(syntheticCard(0).lifecycle);
  });
});
