import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";

import type { ReviewCandidateDto, ReviewItemDto } from "../../bindings";
import { ScrapeRequestError, tauriScrapeBackend } from "./scrapeBackend";
import { ReviewRequestError, tauriReviewBackend } from "./reviewBackend";

afterEach(() => {
  clearMocks();
});

type Call = { cmd: string; args: Record<string, unknown> | undefined };

function record(answer: (cmd: string, args: Record<string, unknown> | undefined) => unknown) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> | undefined });
    return answer(cmd, args as Record<string, unknown> | undefined);
  });
  return calls;
}

const candidate: ReviewCandidateDto = {
  provider: "tmdb",
  providerId: "593",
  title: "Solaris",
  originalTitle: "Solyaris",
  year: 1972,
  director: "Andrei Tarkovsky",
  runtimeMinutes: 167,
  language: null,
  overview: null,
  posterKey: null,
  // JSON cannot carry a number that is not finite: the generated types call it `null`.
  confidence: null,
  rank: null,
  evidence: [{ kind: "title", verdict: "agree", text: "title 1.00" }],
  breakdown: { similarity: 1, yearPenalty: null, posterPenalty: 0, evidence: 0.05 },
};

const item: ReviewItemDto = {
  id: 4,
  itemId: "17",
  kind: "movie",
  name: "Solaris/Solaris.mkv",
  path: "F:/Movies/Solaris",
  reason: "2 strong candidates",
  attention: true,
  note: null,
  tied: true,
  shortPath: "Movies › Solaris",
  local: {
    title: "Solaris",
    year: null,
    runtimeSeconds: 10012,
    techSummary: "1080p · AVC · FLAC 1.0",
    audioLanguage: "ru",
    ids: [],
    hasNfo: false,
    frameKey: null,
    frameAtSeconds: null,
  },
  candidates: [candidate],
};

describe("the review backend of the desktop app", () => {
  it("lists the queue and turns the numbers JSON could not carry into zeros", async () => {
    record(() => [item]);
    const items = await tauriReviewBackend.list();
    expect(items).toHaveLength(1);
    expect(items[0]?.itemId).toBe("17");
    expect(items[0]?.candidates[0]?.confidence).toBe(0);
    expect(items[0]?.candidates[0]?.breakdown).toEqual({
      similarity: 1,
      yearPenalty: 0,
      posterPenalty: 0,
      evidence: 0.05,
    });
  });

  it("sends the decisions to the matching commands", async () => {
    const calls = record((cmd) => {
      if (cmd === "accept_top_review_candidates") return [4, 5];
      if (cmd === "match_library_items") return 3;
      if (cmd === "start_match_batch") return { planned: 5, withoutProvider: 1, jobIds: [1] };
      if (cmd === "count_review_items") return 9;
      if (cmd === "search_review_candidates") return [candidate];
      return null;
    });
    await tauriReviewBackend.accept(4, { provider: "tmdb", providerId: "593" });
    expect(await tauriReviewBackend.acceptTop([4, 5])).toEqual([4, 5]);
    await tauriReviewBackend.ignore(6);
    await tauriReviewBackend.reopen([6]);
    expect(await tauriReviewBackend.count()).toBe(9);
    expect(await tauriReviewBackend.search(4, "solaris")).toHaveLength(1);
    expect(await tauriReviewBackend.matchItems?.(["1", "2", "3"])).toBe(3);
    expect(await tauriReviewBackend.startBatch?.("rescrape_by_ids", ["1"])).toEqual({
      planned: 5,
      withoutProvider: 1,
    });
    expect(calls.map((call) => call.cmd)).toEqual([
      "accept_review_candidate",
      "accept_top_review_candidates",
      "ignore_review_item",
      "reopen_review_items",
      "count_review_items",
      "search_review_candidates",
      "match_library_items",
      "start_match_batch",
    ]);
    expect(calls[0]?.args).toEqual({ reviewId: 4, provider: "tmdb", providerId: "593" });
    expect(calls[6]?.args).toEqual({ itemIds: ["1", "2", "3"] });
  });

  it("reports a refusal with its code and message", async () => {
    mockIPC(() => {
      throw { code: "locked", message: "This title is locked, so it was not changed." };
    });
    const error = await tauriReviewBackend.ignore(1).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ReviewRequestError);
    expect((error as ReviewRequestError).code).toBe("locked");
    expect((error as ReviewRequestError).message).toContain("locked");
  });
});

describe("the scrape backend of the desktop app", () => {
  const target = { itemId: "17", kind: "movie", title: "Solaris", year: null } as const;

  it("applies a scrape through the command and undoes it with the token it answered", async () => {
    const calls = record((cmd) => (cmd === "apply_scrape" ? { undo: "-3" } : null));
    const applied = await tauriScrapeBackend.scrape(target, {
      provider: "tmdb",
      providerId: "2103",
      language: "en",
      fields: ["title", "plot"],
      doNotOverwrite: true,
      secondVersion: true,
    });
    expect(applied.undo).toBe("-3");
    await tauriScrapeBackend.undo("-3");
    expect(calls.map((call) => call.cmd)).toEqual(["apply_scrape", "undo_scrape"]);
    expect(calls[0]?.args).toMatchObject({
      itemId: "17",
      request: { providerId: "2103", doNotOverwrite: true, secondVersion: true },
    });
    expect(calls[1]?.args).toEqual({ token: "-3" });
  });

  it("asks the backend which fields a preset means", async () => {
    const calls = record(() => ({ fields: ["poster"], doNotOverwrite: false }));
    const selection = await tauriScrapeBackend.selection("movie", "artwork_only", []);
    expect(selection.fields).toEqual(["poster"]);
    expect(calls[0]).toMatchObject({
      cmd: "scrape_field_selection",
      args: { kind: "movie", preset: "artwork_only", custom: [] },
    });
  });

  it("reports a refusal as a scrape error", async () => {
    mockIPC(() => {
      throw { code: "provider", message: "TMDB is limiting requests." };
    });
    const error = await tauriScrapeBackend
      .scrape(target, {
        provider: "tmdb",
        providerId: "1",
        language: "en",
        fields: [],
        doNotOverwrite: false,
        secondVersion: false,
      })
      .catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ScrapeRequestError);
    expect((error as ScrapeRequestError).code).toBe("provider");
  });
});
