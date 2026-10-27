import type { MessageKey } from "../../i18n/catalog";

import type { LibrarySortField } from "./libraryBackend";

/** The columns of the Library table in their default order; Stage is the 34px column after the checkbox. */
const LIBRARY_COLUMN_IDS = [
  "stage",
  "title",
  "year",
  "quality",
  "size",
  "folder",
  "rating",
  "runtime",
  "added",
  "modified",
] as const;

export type LibraryColumnId = (typeof LIBRARY_COLUMN_IDS)[number];

export const COLUMN_LABEL: Readonly<Record<LibraryColumnId, MessageKey>> = {
  stage: "library.column.stage",
  title: "library.column.title",
  year: "library.column.year",
  quality: "library.column.quality",
  size: "library.column.size",
  folder: "library.column.folder",
  rating: "library.column.rating",
  runtime: "library.column.runtime",
  added: "library.column.added",
  modified: "library.column.modified",
};

/** The columns the Columns menu can switch: the title is always shown. */
export const SWITCHABLE_COLUMNS: readonly LibraryColumnId[] = LIBRARY_COLUMN_IDS.filter(
  (id) => id !== "title",
);

/** The columns that start switched off: the ones a fresh user rarely needs. */
export const DEFAULT_HIDDEN_COLUMNS: readonly LibraryColumnId[] = ["added", "modified"];

/**
 * What a click on a column header sorts by, in order: every column has a backend sort, so the
 * whole library is ordered, not the rows that happen to be loaded. Quality's resolution rank
 * already counts HDR, and ties always fall back to the title (`titleQuery`).
 */
export const COLUMN_SORT: Readonly<Record<LibraryColumnId, LibrarySortField>> = {
  stage: "stage",
  title: "title",
  year: "year",
  quality: "quality",
  size: "total_file_size",
  folder: "folder",
  rating: "rating",
  runtime: "runtime",
  added: "date_added",
  modified: "file_modified",
};

/** Columns where the biggest or newest value is what a first click should show first. */
export const DESCENDING_FIRST: ReadonlySet<LibraryColumnId> = new Set([
  "quality",
  "size",
  "rating",
  "runtime",
  "added",
  "modified",
]);

/** The column a sort field belongs to, or null for a field no column has. */
export function columnOfField(field: LibrarySortField): LibraryColumnId | null {
  for (const id of LIBRARY_COLUMN_IDS) {
    if (COLUMN_SORT[id] === field) return id;
  }
  return null;
}
