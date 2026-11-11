import { create } from "zustand";

import type { LibraryChip, LibraryFacet, LibraryQuery, LibrarySortKey } from "./libraryBackend";
import { POSTER_SIZES } from "./gridGeometry";
import type { PosterSize } from "./gridGeometry";
import { DEFAULT_HIDDEN_COLUMNS } from "./libraryColumns";

/** The three things the Library switches between (`/library?type=`). */
export type LibraryType = "movies" | "tv" | "collections";
/** The two libraries that have a grid, a table and filters. */
export type TitleType = Exclude<LibraryType, "collections">;

export type LibraryView = "grid" | "table";
export type StageChoice = "all" | "new" | "scraped" | "renamed";
export type QualityChoice = "any" | "2160p" | "1080p" | "720p" | "sd";
export type ProblemChoice = "any" | "all" | "not-matched" | "uncertain" | "missing";
/**
 * One thing a title can be missing, from the "Needs fixing" menu. Several together keep the titles
 * missing all of them. "Not scraped" is not one of these: it is the New stage chip.
 */
export type FixChoice = "poster" | "fanart" | "clearlogo" | "plot" | "title";

/** The dropdown and stage filters of one library; typed terms come from the address (`?q=`). */
export interface LibraryFilters {
  stage: StageChoice;
  /** A library folder label, or null for all of them. */
  drive: string | null;
  quality: QualityChoice;
  problems: ProblemChoice;
  /** Only the movies in open groups of copies (`movie-copies`); movies only. */
  copies: boolean;
  /** What the listed titles must be missing, from the Needs fixing menu. */
  fix: readonly FixChoice[];
}

const NO_FILTERS: LibraryFilters = {
  stage: "all",
  drive: null,
  quality: "any",
  problems: "any",
  copies: false,
  fix: [],
};

const DEFAULT_SORT: readonly LibrarySortKey[] = [{ field: "title", descending: false }];

interface TypeState {
  view: LibraryView;
  sort: readonly LibrarySortKey[];
  filters: LibraryFilters;
  /** Table columns the user switched off. */
  hiddenColumns: readonly string[];
}

const FRESH: TypeState = {
  view: "grid",
  sort: DEFAULT_SORT,
  filters: NO_FILTERS,
  hiddenColumns: DEFAULT_HIDDEN_COLUMNS,
};

interface LibraryStore {
  /** The last type the user looked at: opens when the address names none. */
  type: LibraryType;
  movies: TypeState;
  tv: TypeState;
  /** How wide the posters of the grid are; one choice for Movies and TV shows. */
  posterSize: PosterSize;
  setPosterSize: (size: PosterSize) => void;
  setType: (type: LibraryType) => void;
  setView: (type: TitleType, view: LibraryView) => void;
  setSort: (type: TitleType, sort: readonly LibrarySortKey[]) => void;
  setFilters: (type: TitleType, change: Partial<LibraryFilters>) => void;
  clearFilters: (type: TitleType) => void;
  setColumnHidden: (type: TitleType, column: string, hidden: boolean) => void;
}

const STORAGE_KEY = "cinevault.library.v1";

/**
 * What survives a restart: the type, and per type the view, sort and columns. Filters do not.
 * `columns` is the version of the column list: a save without it predates the Date added and File
 * modified columns, which then start switched off for it too.
 */
interface Saved {
  type?: LibraryType;
  posterSize?: unknown;
  columns?: number;
  movies?: Partial<Pick<TypeState, "view" | "sort" | "hiddenColumns">>;
  tv?: Partial<Pick<TypeState, "view" | "sort" | "hiddenColumns">>;
}

function isType(value: unknown): value is LibraryType {
  return value === "movies" || value === "tv" || value === "collections";
}

function readSaved(): Saved {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return {};
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === "object" && parsed !== null ? (parsed as Saved) : {};
  } catch {
    return {};
  }
}

function sanitizeSort(value: unknown): readonly LibrarySortKey[] {
  if (!Array.isArray(value)) return DEFAULT_SORT;
  const keys = value.filter(
    (key): key is LibrarySortKey =>
      typeof key === "object" &&
      key !== null &&
      typeof (key as LibrarySortKey).field === "string" &&
      typeof (key as LibrarySortKey).descending === "boolean",
  );
  return keys.length > 0 ? keys : DEFAULT_SORT;
}

const COLUMNS_VERSION = 2;

function stateFrom(saved: Saved["movies"], columnsVersion: number | undefined): TypeState {
  const hidden = Array.isArray(saved?.hiddenColumns)
    ? saved.hiddenColumns.filter((id): id is string => typeof id === "string")
    : null;
  return {
    ...FRESH,
    view: saved?.view === "table" ? "table" : "grid",
    sort: sanitizeSort(saved?.sort),
    hiddenColumns:
      hidden === null
        ? FRESH.hiddenColumns
        : columnsVersion === COLUMNS_VERSION
          ? hidden
          : [...new Set([...hidden, ...DEFAULT_HIDDEN_COLUMNS])],
  };
}

function isPosterSize(value: unknown): value is PosterSize {
  return typeof value === "string" && value in POSTER_SIZES;
}

type Persisted = Pick<LibraryStore, "type" | "movies" | "tv" | "posterSize">;

function initial(): Persisted {
  const saved = readSaved();
  return {
    posterSize: isPosterSize(saved.posterSize) ? saved.posterSize : "m",
    type: isType(saved.type) ? saved.type : "movies",
    movies: stateFrom(saved.movies, saved.columns),
    tv: stateFrom(saved.tv, saved.columns),
  };
}

function save(state: Persisted): void {
  const part = (s: TypeState) => ({ view: s.view, sort: s.sort, hiddenColumns: s.hiddenColumns });
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        type: state.type,
        posterSize: state.posterSize,
        columns: COLUMNS_VERSION,
        movies: part(state.movies),
        tv: part(state.tv),
      }),
    );
  } catch {
    // The choices then last for this visit only.
  }
}

export const useLibraryStore = create<LibraryStore>((set, get) => {
  const update = (change: (store: LibraryStore) => Partial<LibraryStore>) => {
    set(change(get()));
    save(get());
  };
  const patch = (type: TitleType, change: Partial<TypeState>) =>
    update((store) => ({ [type]: { ...store[type], ...change } }));
  return {
    ...initial(),
    setType: (type) => update(() => ({ type })),
    setPosterSize: (posterSize) => update(() => ({ posterSize })),
    setView: (type, view) => patch(type, { view }),
    setSort: (type, sort) => patch(type, { sort }),
    setFilters: (type, change) =>
      update((store) => ({
        [type]: { ...store[type], filters: { ...store[type].filters, ...change } },
      })),
    clearFilters: (type) => patch(type, { filters: NO_FILTERS }),
    setColumnHidden: (type, column, hidden) =>
      update((store) => {
        const rest = store[type].hiddenColumns.filter((id) => id !== column);
        return { [type]: { ...store[type], hiddenColumns: hidden ? [...rest, column] : rest } };
      }),
  };
});

/** Back to the factory state; tests call it between runs. */
export function resetLibraryStore(): void {
  useLibraryStore.setState({ ...initialFresh() });
}

function initialFresh(): Persisted {
  return { type: "movies", movies: FRESH, tv: FRESH, posterSize: "m" };
}

// ---- Filters to a query -------------------------------------------------------------------------

const chip = (field: LibraryChip["field"], values: string[]): LibraryChip => ({
  field,
  negated: false,
  values,
  min: null,
  max: null,
});

const flag = (field: LibraryChip["field"]): LibraryChip => chip(field, []);

/** The chips a problem choice stands for; "all" is the union of the three flags. */
export function problemChips(problems: ProblemChoice): LibraryChip[][] {
  switch (problems) {
    case "any":
      return [];
    case "not-matched":
      return [[flag("not_matched")]];
    case "uncertain":
      return [[flag("review")]];
    case "missing":
      return [[flag("missing")]];
    case "all":
      return [[flag("not_matched")], [flag("review")], [flag("missing")]];
  }
}

/** The catalogue filter and value each Needs fixing choice stands for. */
export const FIX_FACETS: Readonly<Record<FixChoice, { id: string; value: string }>> = {
  poster: { id: "missing_artwork", value: "Poster" },
  fanart: { id: "missing_artwork", value: "Fan art" },
  clearlogo: { id: "missing_artwork", value: "Clear logo" },
  plot: { id: "missing_metadata", value: "Plot" },
  title: { id: "missing_metadata", value: "Title" },
};

/** The catalogue chips of the Needs fixing choices: one per filter, needing all of its values. */
export function fixFacets(fix: readonly FixChoice[]): LibraryFacet[] {
  const byId = new Map<string, string[]>();
  for (const choice of fix) {
    const { id, value } = FIX_FACETS[choice];
    byId.set(id, [...(byId.get(id) ?? []), value]);
  }
  return [...byId].map(([id, values]) => ({
    id,
    negated: false,
    mode: "all",
    op: "eq",
    values,
    number: null,
    number2: null,
  }));
}

const QUALITY_VALUES: Readonly<Record<Exclude<QualityChoice, "any">, string[]>> = {
  "2160p": ["2160p"],
  "1080p": ["1080p"],
  "720p": ["720p"],
  sd: ["576p", "480p"],
};

/** The chips of the stage, drive and quality filters. */
export function filterChips(filters: LibraryFilters): LibraryChip[] {
  const chips: LibraryChip[] = [];
  if (filters.stage !== "all") chips.push(chip("stage", [filters.stage]));
  if (filters.drive !== null) chips.push(chip("library_source", [filters.drive]));
  if (filters.quality !== "any") chips.push(chip("resolution", QUALITY_VALUES[filters.quality]));
  if (filters.copies) chips.push(flag("copies"));
  return chips;
}

/** The sort with the title as the final tie-breaker, so equal rows never shuffle. */
function withTitleLast(sort: readonly LibrarySortKey[]): LibrarySortKey[] {
  const tieBreak: LibrarySortKey = { field: "title", descending: false };
  return [...sort, tieBreak].filter(
    (key, index, all) => all.findIndex((other) => other.field === key.field) === index,
  );
}

/** The query for a title library with the given extra chips (typed terms, one problem flag). */
export function titleQuery(
  type: TitleType,
  sort: readonly LibrarySortKey[],
  chips: readonly LibraryChip[],
  text: string,
  ratingSource: string | null,
  facets: readonly LibraryFacet[] = [],
): LibraryQuery {
  return {
    kind: type === "movies" ? "movies" : "shows",
    sort: withTitleLast(sort),
    search: text,
    filters: [...chips],
    ...(facets.length > 0 ? { facets: [...facets] } : {}),
    rating_source: ratingSource,
  };
}
