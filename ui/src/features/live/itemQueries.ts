import type { QueryKey } from "@tanstack/react-query";

import type { ChangePartDto } from "../../bindings";

/** A part of a title that can change: ratings, artwork, details, files or stage. */
export type ChangePart = ChangePartDto;

/**
 * The query key of one part of one title. New queries put the part they show in the key
 * (`["item", id, "ratings"]`), so a ratings event refreshes the ratings strip and nothing else.
 */
export function itemPartKey(id: string, part: ChangePart): QueryKey {
  return ["item", id, part];
}

/**
 * The older pages read a whole title in one query, so any change of a part also refreshes the
 * query that holds it. These keys go once the pages read their parts separately.
 */
const WHOLE_TITLE_QUERIES = ["movie-detail", "tv-show", "tv-episode"] as const;

/** The keys to invalidate for one title when `part` changed. */
export function itemKeys(id: string, part: ChangePart): readonly QueryKey[] {
  return [itemPartKey(id, part), ...WHOLE_TITLE_QUERIES.map((name): QueryKey => [name, id])];
}

/**
 * The keys of what lists and counts show about many titles. A change of any part can alter a row
 * (a rating column, a poster, the stage), so rows are refreshed; the pipeline counts and the
 * sidebar numbers follow the stage and the fields that decide the review count.
 */
export function listKeys(parts: ReadonlySet<ChangePart>): readonly QueryKey[] {
  const keys: QueryKey[] = [["library", "window"]];
  if (parts.has("stage") || parts.has("details")) {
    keys.push(["lifecycle"], ["library", "counts"], ["review", "count"]);
  }
  // The Organize page plans and lists its history again when names change on disk.
  if (parts.has("stage") || parts.has("files")) keys.push(["organize"]);
  // Home's size, runtime and breakdowns and the missing and duplicate review follow the files.
  if (parts.has("stage") || parts.has("files") || parts.has("details")) {
    keys.push(["home"], ["library-ops", "review"]);
  }
  return keys;
}
