import type { MessageKey } from "../../i18n/catalog";
import type { LibrarySortKey } from "./libraryBackend";

/** The sorts the sort menu offers; a click on a table header can make others ("Custom"). */
export type SortChoice =
  | "titleAsc"
  | "titleDesc"
  | "added"
  | "addedAsc"
  | "modifiedDesc"
  | "modifiedAsc"
  | "yearDesc"
  | "yearAsc"
  | "ratingDesc"
  | "runtimeDesc";

export const SORT_CHOICES: readonly SortChoice[] = [
  "titleAsc",
  "titleDesc",
  "added",
  "addedAsc",
  "modifiedDesc",
  "modifiedAsc",
  "yearDesc",
  "yearAsc",
  "ratingDesc",
  "runtimeDesc",
];

export const SORT_KEYS: Readonly<Record<SortChoice, readonly LibrarySortKey[]>> = {
  titleAsc: [{ field: "title", descending: false }],
  titleDesc: [{ field: "title", descending: true }],
  added: [{ field: "date_added", descending: true }],
  addedAsc: [{ field: "date_added", descending: false }],
  modifiedDesc: [{ field: "file_modified", descending: true }],
  modifiedAsc: [{ field: "file_modified", descending: false }],
  yearDesc: [{ field: "year", descending: true }],
  yearAsc: [{ field: "year", descending: false }],
  ratingDesc: [{ field: "rating", descending: true }],
  runtimeDesc: [{ field: "runtime", descending: true }],
};

export const SORT_LABEL: Readonly<Record<SortChoice, MessageKey>> = {
  titleAsc: "library.sortBy.titleAsc",
  titleDesc: "library.sortBy.titleDesc",
  added: "library.sortBy.added",
  addedAsc: "library.sortBy.addedAsc",
  modifiedDesc: "library.sortBy.modifiedDesc",
  modifiedAsc: "library.sortBy.modifiedAsc",
  yearDesc: "library.sortBy.yearDesc",
  yearAsc: "library.sortBy.yearAsc",
  ratingDesc: "library.sortBy.ratingDesc",
  runtimeDesc: "library.sortBy.runtimeDesc",
};

/** The menu choice a sort is, or null for a sort made from a table header. */
export function sortChoiceOf(sort: readonly LibrarySortKey[]): SortChoice | null {
  const first = sort[0];
  if (first === undefined || sort.length !== 1) return null;
  return (
    SORT_CHOICES.find((choice) => {
      const keys = SORT_KEYS[choice];
      return keys[0]?.field === first.field && keys[0].descending === first.descending;
    }) ?? null
  );
}
