import { useMemo } from "react";

import type { IndexRange, Listing } from "../library/useLibraryListing";
import { useDeleteStore } from "./deleteActions";

/**
 * The listing without the titles Delete is hiding (spec `title-deletion`: the title leaves the
 * library at once). The rows after a hidden one move up; the selection, the footer count and the
 * range the list asks for follow the same numbering. With nothing hidden it is the listing itself.
 */
export function withoutHidden(listing: Listing, hidden: ReadonlySet<string>): Listing {
  if (hidden.size === 0) return listing;
  const shown: number[] = [];
  listing.keys.forEach((key, index) => {
    if (!hidden.has(key)) shown.push(index);
  });
  if (shown.length === listing.keys.length) return listing;
  const last = Math.max(shown.length - 1, 0);
  const under = (index: number): number => shown[Math.min(Math.max(index, 0), last)] ?? 0;
  return {
    ...listing,
    total: shown.length,
    keys: shown.map((index) => listing.keys[index] ?? ""),
    rowAt: (index) => (index < shown.length ? listing.rowAt(under(index)) : undefined),
    setRange: (range: IndexRange) =>
      listing.setRange({ start: under(range.start), end: under(range.end) }),
    version: `${listing.version}|${[...hidden].join(",")}`,
  };
}

/** [`withoutHidden`] for the titles Delete hides now; re-renders when that set changes. */
export function useListingWithoutHidden(listing: Listing): Listing {
  const hidden = useDeleteStore((state) => state.hidden);
  return useMemo(
    () => withoutHidden(listing, hidden),
    // The listing is a new object on every render; its version says when its content changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [listing.version, listing.keys, listing.total, hidden],
  );
}
