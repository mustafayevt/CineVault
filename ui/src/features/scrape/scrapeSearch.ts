/** The search of `/scrape`: `ids=1,2,3` limits the page to the titles the Library sent. */
export interface ScrapeSearch {
  ids?: string;
}

/** Reads the search of `/scrape`; a lone id arrives as a number, anything else is dropped. */
export function validateScrapeSearch(search: Record<string, unknown>): ScrapeSearch {
  const raw = search.ids;
  const text = typeof raw === "number" ? String(raw) : typeof raw === "string" ? raw : "";
  return text.trim() === "" ? {} : { ids: text.trim() };
}

/** The ids of a search, without blanks or repeats; `null` when there are none. */
export function scopeIdsOf(search: ScrapeSearch): string[] | null {
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
