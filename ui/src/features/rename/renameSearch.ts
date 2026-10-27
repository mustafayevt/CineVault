/** The search of `/rename`: `ids=1,2,3` limits the page to those titles. */
export interface RenameSearch {
  ids?: string;
}

/** Reads the search of `/rename`. A lone id arrives as a number; anything else is dropped. */
export function validateRenameSearch(search: Record<string, unknown>): RenameSearch {
  const raw = search.ids;
  const text = typeof raw === "number" ? String(raw) : typeof raw === "string" ? raw : "";
  return text.trim() === "" ? {} : { ids: text.trim() };
}

/** The ids of a search, without blanks or repeats; `null` when there are none. */
export function selectionOf(search: RenameSearch): string[] | null {
  if (search.ids === undefined) return null;
  const ids = [
    ...new Set(
      search.ids
        .split(",")
        .map((id) => id.trim())
        .filter((id) => id !== ""),
    ),
  ];
  return ids.length === 0 ? null : ids;
}
