import { useQuery } from "@tanstack/react-query";

import { currentCopiesBackend } from "./copiesBackend";

/** Query key of the copies of a movie. */
export function copiesOfKey(id: string) {
  return ["copies", "of", id] as const;
}

/** Query key of a movie's Compare copies page. */
export function comparisonKey(id: string) {
  return ["copies", "compare", id] as const;
}

/**
 * The copies of a movie, kept or not (`null` when it has none): the Movie page's "You have 2
 * copies" line. A failed read just means no line.
 */
export function useCopiesOf(id: string) {
  return useQuery({
    queryKey: copiesOfKey(id),
    queryFn: () => currentCopiesBackend().copiesOf(id),
    staleTime: 30_000,
  });
}
