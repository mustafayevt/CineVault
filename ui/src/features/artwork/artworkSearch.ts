/**
 * The search of `/artwork`: `ids=1,2,3` are the titles to choose artwork for (one title opens the
 * picker on it, several step through them), `type=` the artwork type it opens on, and
 * `collection=` a collection whose poster and fanart are changed.
 */
export interface ArtworkSearch {
  ids?: string;
  type?: string;
  collection?: string;
}

/** Reads the search of `/artwork`; a lone id arrives as a number, anything else is dropped. */
export function validateArtworkSearch(search: Record<string, unknown>): ArtworkSearch {
  const text = (value: unknown): string =>
    typeof value === "number" ? String(value) : typeof value === "string" ? value.trim() : "";
  const ids = text(search.ids);
  const type = text(search.type);
  const collection = text(search.collection);
  return {
    ...(ids === "" ? {} : { ids }),
    ...(type === "" ? {} : { type }),
    ...(collection === "" ? {} : { collection }),
  };
}

/** The ids of a search, without blanks or repeats; empty when there are none. */
export function artworkIdsOf(search: ArtworkSearch): string[] {
  if (search.ids === undefined) return [];
  return [
    ...new Set(
      search.ids
        .split(",")
        .map((id) => id.trim())
        .filter((id) => id !== ""),
    ),
  ];
}
