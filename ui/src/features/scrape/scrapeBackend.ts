import { isTauri } from "@tauri-apps/api/core";
import type { Channel } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  AcceptConfidentDto,
  FieldDiffDto,
  MatchCommandError,
  PreviewCandidateDto,
  ProviderEvent,
  ReviewCandidateDto,
  ScrapeLinkDto,
  ScrapePresetDto,
  ScrapePreviewStartDto,
  ScrapeProviderDto,
  ScrapeRequestDto,
  ScrapeSelectionDto,
  ScrapeStreamHitDto,
  ScrapeStreamStartDto,
} from "../../bindings";
import { SCRAPE_FIELDS } from "../settings/schema";
import { channelTransport } from "../workspaces/channelTransport";
import type { ProviderSearchTransport } from "../workspaces/providerEvents";
import { createSyntheticScrapeBackend } from "./syntheticScrape";

/** The kinds of item the workspace can scrape; episodes are scraped with their show. */
export type ScrapeKind = "movie" | "show";

/** The library item the workspace was opened for. */
export interface ScrapeTarget {
  itemId: string;
  kind: ScrapeKind;
  /** The title to start the search with. */
  title: string;
  year: number | null;
}

/** A provider that answers the search, as the stream names it. */
export type ScrapeProviderInfo = ScrapeProviderDto;

/** A library item that is already linked to a title. */
export type LibraryLink = ScrapeLinkDto;

/** A refusal from the backend: `message` is ready to show, `code` says why. */
export class ScrapeRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ScrapeRequestError";
    this.code = code;
  }
}

/** What applying a scrape answers: the token that puts the title back as it was. */
export interface ScrapeApplied {
  /** Pass to `undo`; `null` when the scrape cannot be undone or History records it. */
  undo: string | null;
  /**
   * The History entry of a change of match (a scraped title given another match): Undo goes
   * through History. `null` for a first scrape or a scrape with the same match.
   */
  historyEntry: string | null;
}

/** What the search-only pass found for a title: the candidates, best first, and the confident flag. */
export interface SearchOnlyResult {
  hits: ScrapeStreamHit[];
  /** The top candidate scores 90% or more and is not tied (the backend decides). */
  confident: boolean;
}

/** What Accept all confident did, by library item id. */
export interface AcceptConfidentResult {
  /** `undo` is the token of `undo`, or the review row to reopen when the title waited in one. */
  accepted: { itemId: string; undo: string | null; reviewId: number | null }[];
  /** The one History entry of the run: its Undo takes every accepted title back. */
  historyEntry: string | null;
  /** No confident top candidate: left as it was. */
  skipped: string[];
  /** Failed or refused (locked): left as it was. */
  failed: string[];
}

/** Where the workspace gets its searches and how a scrape is applied. */
export interface ScrapeBackend {
  /** False when nothing scrapes here, so the entry points stay disabled. */
  readonly available: boolean;
  /**
   * The streaming search of the workspace: candidates from every metadata
   * provider, merged by shared IDs. `onStart` receives the providers that will answer.
   */
  searchTransport: (
    onStart?: (start: ScrapeStreamStartDto) => void,
  ) => ProviderSearchTransport<ScrapeStreamHit>;
  /** The streaming preview of one chosen result: the diff of its fields, in two phases. */
  previewTransport: (
    onStart?: (start: ScrapePreviewStartDto) => void,
  ) => ProviderSearchTransport<ScrapeFieldDiff>;
  /** The field keys and the do-not-overwrite mode a preset means. */
  selection: (
    kind: ScrapeKind,
    preset: ScrapePreset,
    custom: readonly string[],
  ) => Promise<ScrapeSelection>;
  /** Links the item to the result and writes the chosen fields. */
  scrape: (target: ScrapeTarget, request: ScrapeRequest) => Promise<ScrapeApplied>;
  /** Puts the title back as it was before a scrape. */
  undo: (token: string) => Promise<void>;
  /**
   * The search-only pass for a New title (spec `title-matching`): candidates scored against the
   * file, nothing written. An empty `query` searches by the title's own name.
   */
  searchOnly: (
    itemId: string,
    kind: ScrapeKind,
    query: string,
    language: string,
  ) => Promise<SearchOnlyResult>;
  /** The title the item is linked to as a scored candidate; `null` when it has no usable ID. */
  currentMatch: (itemId: string, language: string) => Promise<ScrapeStreamHit | null>;
  /** Scrapes the titles whose top candidate is confident, with that candidate. */
  acceptConfident: (itemIds: readonly string[]) => Promise<AcceptConfidentResult>;
}

/** What to scrape and how. */
export type ScrapeRequest = ScrapeRequestDto;

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: MatchCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ScrapeRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

/**
 * One entry of the streaming search: a title that one or more providers found.
 * An entry sent again with the same `id` replaces the one shown; `absorbs` names entries that
 * were joined into it and are to be dropped.
 */
export type ScrapeStreamHit = ScrapeStreamHitDto;

/** A candidate of the backend as an entry of the page's candidate lists. */
export function hitOfCandidate(candidate: ReviewCandidateDto, prefix: string): ScrapeStreamHit {
  return {
    id: `${prefix}:${candidate.provider}:${candidate.providerId}`,
    candidate,
    providers: [
      {
        provider: candidate.provider,
        providerId: candidate.providerId,
        confidence: candidate.confidence,
      },
    ],
    ids: [],
    inLibrary: null,
    absorbs: [],
  };
}

/** The provider that carries the reason a search found nothing, as a `failed` event. */
export const SEARCH_PROVIDER = "search";

/**
 * The streaming search of the Scrape workspace: `params` of the request carry the `itemId`, the
 * `kind` (`movie` or `show`) and the `language`; `query` is the typed text. `onStart` receives the
 * providers that will answer.
 */
function scrapeStreamTransport(
  onStart?: (start: ScrapeStreamStartDto) => void,
): ProviderSearchTransport<ScrapeStreamHit> {
  return channelTransport<ScrapeStreamHit>(async (request, channel) => {
    const start = unwrap(
      await commands.streamScrapeCandidates(
        String(request.params?.itemId ?? ""),
        request.query,
        request.params?.kind === "show" ? "show" : "movie",
        String(request.params?.language ?? ""),
        request.searchId,
        channel as Channel<ProviderEvent<ScrapeStreamHitDto>>,
      ),
    );
    onStart?.(start);
  });
}

/** One field of the stored title next to the chosen result's: what applying would do to it. */
export type ScrapeFieldDiff = FieldDiffDto;

/** The result the user chose, as the search showed it. */
export type ScrapePreviewCandidate = PreviewCandidateDto;

/** Everything / Only empty / Artwork only / Custom. */
export type ScrapePreset = ScrapePresetDto;

/** The field keys and the do-not-overwrite mode a preset means; pass them to `scrape`. */
export type ScrapeSelection = ScrapeSelectionDto;

/** The sources of a streaming preview: the search result's fields, then the provider's details. */
export const PREVIEW_HIT_SOURCE = "hit";
export const PREVIEW_DETAILS_SOURCE = "details";

/**
 * The streaming preview of the Scrape workspace: `params` of the request carry the
 * `itemId`, the `language` and the chosen result as JSON in `candidate`. The diff of the search
 * result's fields arrives first (source `hit`); the diff of every field follows (source
 * `details`) and replaces the first rows by `field`.
 */
function scrapePreviewTransport(
  onStart?: (start: ScrapePreviewStartDto) => void,
): ProviderSearchTransport<ScrapeFieldDiff> {
  return channelTransport<ScrapeFieldDiff>(async (request, channel) => {
    const candidate = JSON.parse(
      String(request.params?.candidate ?? "{}"),
    ) as ScrapePreviewCandidate;
    const start = unwrap(
      await commands.streamScrapePreview(
        String(request.params?.itemId ?? ""),
        candidate,
        String(request.params?.language ?? ""),
        request.searchId,
        channel as Channel<ProviderEvent<FieldDiffDto>>,
      ),
    );
    onStart?.(start);
  });
}

const ARTWORK_FIELDS: ReadonlySet<string> = new Set([
  "poster",
  "fanart",
  "banner",
  "clearart",
  "disc",
  "clearlogo",
  "thumb",
  "characterart",
  "keyart",
  "squareart",
  "extrafanart",
  "extrathumb",
]);

/** Whether a field key is an image. */
function isArtworkField(field: string): boolean {
  return ARTWORK_FIELDS.has(field);
}

/**
 * What a preset writes for a kind of title, worked out in the page. The desktop app asks the
 * backend (`scrape_field_selection`); this is the same rule, for a plain browser and the tests.
 */
export function localFieldSelection(
  kind: ScrapeKind,
  preset: ScrapePreset,
  custom: readonly string[],
): ScrapeSelection {
  const all: readonly string[] = SCRAPE_FIELDS[kind];
  switch (preset) {
    case "everything":
      return { fields: [...all], doNotOverwrite: false };
    case "only_empty":
      return { fields: [...all], doNotOverwrite: true };
    case "artwork_only":
      return { fields: all.filter(isArtworkField), doNotOverwrite: false };
    case "custom":
      return { fields: all.filter((field) => custom.includes(field)), doNotOverwrite: false };
  }
}

/** The backend of the desktop app: the providers and the scrape live in the Rust service. */
export const tauriScrapeBackend: ScrapeBackend = {
  available: true,
  searchTransport: scrapeStreamTransport,
  previewTransport: scrapePreviewTransport,
  selection: (kind, preset, custom) => commands.scrapeFieldSelection(kind, preset, [...custom]),
  scrape: async (target, request) =>
    unwrap(await commands.applyScrape(target.itemId, request)) as ScrapeApplied,
  undo: async (token) => void unwrap(await commands.undoScrape(token)),
  searchOnly: async (itemId, _kind, query, language) => {
    const found = unwrap(await commands.searchNewTitle(itemId, query, language));
    return {
      hits: found.candidates.map((candidate) => hitOfCandidate(candidate, "found")),
      confident: found.confident,
    };
  },
  currentMatch: async (itemId, language) => {
    const current = unwrap(await commands.currentMatchCandidate(itemId, language));
    return current === null ? null : hitOfCandidate(current, "current");
  },
  acceptConfident: async (itemIds) =>
    acceptResultOf(unwrap(await commands.acceptConfidentTitles([...itemIds]))),
};

function acceptResultOf(done: AcceptConfidentDto): AcceptConfidentResult {
  return {
    accepted: done.accepted.map((entry) => ({
      itemId: entry.itemId,
      undo: entry.undo,
      reviewId: entry.reviewId,
    })),
    historyEntry: done.historyEntry,
    skipped: done.skipped,
    failed: done.failed,
  };
}

let override: ScrapeBackend | null = null;
let fallback: ScrapeBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setScrapeBackend(backend: ScrapeBackend | null): void {
  override = backend;
}

export function currentScrapeBackend(): ScrapeBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriScrapeBackend;
  // A plain browser has no providers: it uses fixtures so the workspace can be developed.
  fallback ??= createSyntheticScrapeBackend({ generic: true });
  return fallback;
}
