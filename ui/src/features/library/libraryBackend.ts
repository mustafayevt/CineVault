import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  FacetValueDto,
  FilterDefDto,
  LibraryCardDto,
  LibraryFacetDto,
  EditCommandError,
  LibraryCommandError,
  LibraryChipDto,
  LibraryCountsDto,
  LibraryKindDto,
  LibraryQueryDto,
  LibraryRowDto,
  LibraryRowsWindowDto,
  LibrarySizeTotalDto,
  LibrarySortFieldDto,
  LibrarySortKeyDto,
  LibraryWindowDto,
  PersonHitDto,
  SourceGroupDto,
} from "../../bindings";
import { restoreWatchedStates } from "../curation/watchedRestore";
import type { WatchedChange } from "../curation/watchedRestore";
import { currentPreferredSource } from "../ratings/preference";
import { createSyntheticBackend } from "./syntheticLibrary";

export type LibraryKind = LibraryKindDto;
export type LibraryCard = LibraryCardDto;
export type LibraryChip = LibraryChipDto;
export type LibraryFacet = LibraryFacetDto;
export type FilterDef = FilterDefDto;
export type FacetValue = FacetValueDto;
export type LibraryQuery = LibraryQueryDto;
export type LibraryWindow = LibraryWindowDto;
export type LibraryCounts = LibraryCountsDto;
export type LibraryRow = LibraryRowDto;
export type LibraryRowsWindow = LibraryRowsWindowDto;
export type LibrarySortField = LibrarySortFieldDto;
export type LibrarySortKey = LibrarySortKeyDto;
export type PersonHit = PersonHitDto;
export type LibrarySizeTotal = LibrarySizeTotalDto;
export type SourceGroup = SourceGroupDto;

/** A refusal from the backend: `message` is ready to show. */
export class LibraryRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "LibraryRequestError";
    this.code = code;
  }
}

/** Where the library comes from: the database in the desktop app, fixtures in a plain browser. */
export interface LibraryBackend {
  /** Rows `offset..offset + limit` of the query, and how many rows match in all. */
  window: (query: LibraryQuery, offset: number, limit: number) => Promise<LibraryWindow>;
  /** Like `window`, with every column of the list and table views. */
  rows: (query: LibraryQuery, offset: number, limit: number) => Promise<LibraryRowsWindow>;
  counts: () => Promise<LibraryCounts>;
  /**
   * The size of the movie or the TV library under the one size rule (each file once, online
   * titles only), and what is offline. Absent where the backend has no sizes.
   */
  sizeTotal?: (kind: Extract<LibraryKind, "movies" | "shows">) => Promise<LibrarySizeTotal>;
  /**
   * The titles of a query grouped by the label of their library folder, in label order: the
   * group headers of the grid, list and table. Absent where the backend has no folders.
   */
  sourceGroups?: (query: LibraryQuery) => Promise<readonly SourceGroup[]>;
  /** The filters of the catalogue that exist for a library, in picker order. */
  filterCatalogue?: (kind: LibraryKind) => Promise<readonly FilterDef[]>;
  /**
   * The values of one list filter and how many items have each, narrowed by the query's chips.
   * The caller leaves the filter's own chip out of the query, so its list does not narrow itself.
   */
  facetValues?: (query: LibraryQuery, facet: string) => Promise<readonly FacetValue[]>;
  /**
   * The actors and directors whose name contains `term` (everyone for an empty term), most
   * credited first, at most `limit`. Absent where the backend has no credits.
   */
  people?: (term: string, limit: number) => Promise<readonly PersonHit[]>;
  /**
   * Flips the watched flag of one item. Absent where the backend cannot change items yet; the
   * card then shows its Watched action as unavailable.
   */
  toggleWatched?: (id: string) => Promise<ToggledWatched>;
  /**
   * Sets the watched flag of many items at once and returns the state each changed item had
   * before (flag, play count, last-watched date), so an Undo can put back exactly those. Absent
   * where the backend cannot change items yet.
   */
  setWatched?: (ids: readonly string[], watched: boolean) => Promise<WatchedChange>;
  /** Puts back the states a `setWatched` or `toggleWatched` reported. */
  restoreWatched?: (previous: WatchedChange) => Promise<void>;
}

/** What flipping one item did: its new state and the state it had before, for Undo. */
export interface ToggledWatched {
  watched: boolean;
  previous: WatchedChange;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: LibraryCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new LibraryRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

/**
 * The query with the preferred rating source filled in when it does not name one, so the rating
 * column, its sort and the `rating` chip follow the Preferred rating setting in every caller.
 */
function withPreferredRating(query: LibraryQuery): LibraryQuery {
  return query.rating_source === undefined || query.rating_source === null
    ? { ...query, rating_source: currentPreferredSource() }
    : query;
}

type EditResult<T> = { status: "ok"; data: T } | { status: "error"; error: EditCommandError };

function unwrapEdit<T>(result: EditResult<T>): T {
  if (result.status === "error") {
    throw new LibraryRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

export const tauriLibraryBackend: LibraryBackend = {
  window: async (query, offset, limit) =>
    unwrap(await commands.listLibraryWindow(withPreferredRating(query), offset, limit)),
  rows: async (query, offset, limit) =>
    unwrap(await commands.listLibraryRows(withPreferredRating(query), offset, limit)),
  counts: async () => unwrap(await commands.libraryCounts()),
  sizeTotal: async (kind) => unwrap(await commands.librarySizeTotal(kind)),
  sourceGroups: async (query) =>
    unwrap(await commands.librarySourceGroups(withPreferredRating(query))),
  filterCatalogue: (kind) => commands.libraryFilterCatalogue(kind),
  facetValues: async (query, facet) =>
    unwrap(await commands.libraryFacetValues(withPreferredRating(query), facet)),
  people: async (term, limit) => unwrap(await commands.searchLibraryPeople(term, limit)),
  toggleWatched: async (id) => {
    const toggled = unwrapEdit(await commands.toggleWatched([id]));
    return { watched: toggled.watched, previous: toggled.report.previous };
  },
  setWatched: async (ids, watched) =>
    unwrapEdit(await commands.setWatched([...ids], watched)).previous,
  restoreWatched: restoreWatchedStates,
};

let override: LibraryBackend | null = null;
let fallback: LibraryBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setLibraryBackend(backend: LibraryBackend | null): void {
  override = backend;
}

/** How many synthetic items a plain browser shows: `?synthetic=20000` in the URL, else 181. */
export function syntheticCountFromLocation(search: string): number {
  const raw = new URLSearchParams(search).get("synthetic");
  const parsed = raw === null ? Number.NaN : Number.parseInt(raw, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? Math.min(parsed, 200_000) : 181;
}

export function currentLibraryBackend(): LibraryBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriLibraryBackend;
  // A plain browser has no database; it renders fixture data so the UI can be developed and traced.
  fallback ??= createSyntheticBackend(syntheticCountFromLocation(window.location.search));
  return fallback;
}
