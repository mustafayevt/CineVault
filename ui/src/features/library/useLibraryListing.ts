import { useQueries, useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";

import { currentLibraryBackend } from "./libraryBackend";
import type { LibraryQuery, LibraryRow, LibrarySortKey } from "./libraryBackend";
import { cardTitle, libraryFolderOf, problemOf } from "./titleFacts";

/** Rows fetched per request while scrolling. */
const PAGE_SIZE = 120;
/** Rows per request when reading every id and size of a listing (the backend caps a window at 500). */
const INDEX_CHUNK = 500;
const PARALLEL = 4;

/** An inclusive range of row indexes on screen. */
export interface IndexRange {
  start: number;
  end: number;
}

/** What an action needs to know about a title the list has shown. */
export interface SeenTitle {
  title: string;
  folder: string;
  filesMissing: boolean;
}

/** What a view needs of the titles it lists. */
export interface Listing {
  /** How many titles match; 0 until the first answer. */
  total: number;
  /** True until the first answer. */
  loading: boolean;
  error: Error | null;
  /** The row at `index`, or `undefined` while its page is loading. */
  rowAt: (index: number) => LibraryRow | undefined;
  /**
   * The id of every title in display order, the keys of the selection. A title whose page has not
   * arrived and whose id is not known yet has a placeholder key, which nothing can select.
   */
  keys: readonly string[];
  /** Tell the listing which rows are on screen, so it loads their pages. */
  setRange: (range: IndexRange) => void;
  /** Changes whenever a page or the id list arrives, so a view can rebuild what it derived. */
  version: string;
  /** The facts of every title that has been on screen, so a selection that scrolled away is known. */
  seen: ReadonlyMap<string, SeenTitle>;
  /** Bytes of one title's files, once known. */
  bytesOf: (id: string) => number | undefined;
  /** Bytes of every listed title; null until every size is known. */
  totalBytes: number | null;
}

/** The pages (0-based) that cover `range`, plus one ahead and one behind. */
function pagesFor(range: IndexRange, total: number): number[] {
  if (total <= 0) return [];
  const last = Math.floor((total - 1) / PAGE_SIZE);
  const first = Math.max(0, Math.floor(range.start / PAGE_SIZE) - 1);
  const through = Math.min(last, Math.floor(Math.max(range.end, 0) / PAGE_SIZE) + 1);
  const pages: number[] = [];
  for (let page = first; page <= through; page += 1) pages.push(page);
  return pages;
}

const bytesInRow = (row: LibraryRow): number => row.total_file_size ?? row.file_size ?? 0;

function pageQuery(query: LibraryQuery, page: number, enabled: boolean) {
  return {
    queryKey: ["library", "rows", query, page] as const,
    queryFn: () => currentLibraryBackend().rows(query, page * PAGE_SIZE, PAGE_SIZE),
    enabled,
    staleTime: Infinity,
    // Pages far from the viewport are dropped after a minute, which bounds memory.
    gcTime: 60_000,
  };
}

/** Every id and size of a listing, read in chunks; the rows are dropped as they arrive. */
async function readIndex(query: LibraryQuery, total: number) {
  const backend = currentLibraryBackend();
  const offsets: number[] = [];
  for (let offset = 0; offset < total; offset += INDEX_CHUNK) offsets.push(offset);
  const chunks: { id: string; bytes: number }[][] = [];
  for (let first = 0; first < offsets.length; first += PARALLEL) {
    const batch = offsets.slice(first, first + PARALLEL);
    const results = await Promise.all(
      batch.map(async (offset) => {
        const window = await backend.rows(query, offset, Math.min(INDEX_CHUNK, total - offset));
        return window.items.map((row) => ({ id: row.card.id, bytes: bytesInRow(row) }));
      }),
    );
    chunks.push(...results);
  }
  const all = chunks.flat();
  return { ids: all.map((entry) => entry.id), bytes: all.map((entry) => entry.bytes) };
}

type SortValue = (row: LibraryRow) => string | number | null;

/** The backend's quality rank: resolution band, HDR on top, unknown height last. */
function qualityRank(row: LibraryRow): number | null {
  if (row.height <= 0) return null;
  const band = row.height >= 1500 ? 8 : row.height >= 760 ? 6 : row.height >= 600 ? 4 : 2;
  return band + (row.hdr_formats.length > 0 ? 1 : 0);
}

/** What the union of several listings sorts by; other fields fall back to the title. */
const SORT_VALUES: Partial<Record<LibrarySortKey["field"], SortValue>> = {
  title: (row) => row.sort_title.toLowerCase(),
  sort_title: (row) => row.sort_title.toLowerCase(),
  year: (row) => (row.card.year > 0 ? row.card.year : null),
  date_added: (row) => row.date_added_ms,
  file_modified: (row) => row.file_modified_ms,
  runtime: (row) => (row.runtime_minutes > 0 ? row.runtime_minutes : null),
  rating: (row) => row.rating,
  path: (row) => row.path.toLowerCase(),
  folder: (row) => libraryFolderOf(row.path).toLowerCase(),
  source: (row) => row.source_label.toLowerCase(),
  video_height: (row) => (row.height > 0 ? row.height : null),
  quality: qualityRank,
  total_file_size: (row) => bytesInRow(row) || null,
  stage: (row) => ["new", "scraped", "renamed"].indexOf(row.card.lifecycle?.stage ?? "new"),
};

/** The rows of several queries as one list without repeats, in the order of `sort`. */
function mergeRows(
  lists: readonly (readonly LibraryRow[])[],
  sort: readonly LibrarySortKey[],
): LibraryRow[] {
  const byId = new Map<string, LibraryRow>();
  for (const list of lists) for (const row of list) byId.set(row.card.id, row);
  const keys: readonly LibrarySortKey[] =
    sort.length > 0 ? sort : [{ field: "title", descending: false }];
  return [...byId.values()].sort((a, b) => {
    for (const key of keys) {
      const value = SORT_VALUES[key.field] ?? SORT_VALUES.title;
      if (value === undefined) continue;
      const left = value(a);
      const right = value(b);
      if (left === right) continue;
      // Like the backend: nothing (null) sorts after every value, in both directions.
      if (left === null || right === null) return left === null ? 1 : -1;
      const order = left < right ? -1 : 1;
      return key.descending ? -order : order;
    }
    return a.card.id < b.card.id ? -1 : 1;
  });
}

/**
 * The titles of a query as a random-access list. Each page of rows is a query-cache
 * entry, so scrolling back is instant; page 0 carries the total, and only the pages around the
 * range on screen are fetched after it. A background read of every id and size (small, and
 * dropped row by row) lets Ctrl+A, Shift ranges and the selection bar's size work on a library
 * the scroll has not reached. With `union` the listing is the titles of any of those queries
 * (the "All problems" filter, which the backend cannot express as one query), read whole.
 */
export function useLibraryListing(
  query: LibraryQuery,
  union: readonly LibraryQuery[] | null = null,
): Listing {
  const windowed = union === null;
  const [range, setRange] = useState<IndexRange>({ start: 0, end: 0 });

  const head = useQuery(pageQuery(query, 0, windowed));
  const total = windowed ? (head.data?.total ?? 0) : 0;
  const pages = pagesFor(range, total).filter((page) => page !== 0);
  const rest = useQueries({ queries: pages.map((page) => pageQuery(query, page, windowed)) });

  const unionQueries = useQueries({
    queries: (union ?? []).map((q) => ({
      queryKey: ["library", "union", q] as const,
      queryFn: () => currentLibraryBackend().rows(q, 0, INDEX_CHUNK),
      staleTime: Infinity,
    })),
  });
  const unionData = unionQueries.map((result) => result.data);
  const unionArrivals = unionQueries.map((result) => result.dataUpdatedAt).join();

  const needsIndex = windowed && total > PAGE_SIZE;
  const index = useQuery({
    queryKey: ["library", "index", query, total] as const,
    queryFn: () => readIndex(query, total),
    enabled: needsIndex,
    staleTime: Infinity,
    gcTime: 60_000,
  });

  const merged = useMemo(
    () =>
      union === null
        ? null
        : mergeRows(
            unionData.flatMap((data) => (data === undefined ? [] : [data.items])),
            query.sort,
          ),
    // The results' identities change when one arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [union, query.sort, unionArrivals],
  );

  const headItems = head.data?.items;
  const pageItems = rest.map((result) => result.data?.items);
  const rowAt = (i: number): LibraryRow | undefined => {
    if (merged !== null) return merged[i];
    const page = Math.floor(i / PAGE_SIZE);
    const items = page === 0 ? headItems : pageItems[pages.indexOf(page)];
    return items?.[i % PAGE_SIZE];
  };

  const indexIds = index.data?.ids;
  const listed = merged !== null ? merged.length : total;
  // The keys change when a page or the id list arrives, not on every render.
  const arrivals = [
    head.dataUpdatedAt,
    index.dataUpdatedAt,
    ...rest.map((r) => r.dataUpdatedAt),
  ].join();
  const keys = useMemo(
    () => {
      if (indexIds !== undefined) return indexIds;
      return Array.from({ length: listed }, (_, i) => rowAt(i)?.card.id ?? `#${i}`);
    },
    // `rowAt` reads the pages whose arrival times are listed here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [indexIds, listed, merged, arrivals],
  );

  const bytes = useMemo(() => {
    const map = new Map<string, number>();
    if (merged !== null) {
      for (const row of merged) map.set(row.card.id, bytesInRow(row));
    } else if (!needsIndex) {
      for (const row of headItems ?? []) map.set(row.card.id, bytesInRow(row));
    } else if (index.data !== undefined) {
      const sizes = index.data.bytes;
      index.data.ids.forEach((id, i) => map.set(id, sizes[i] ?? 0));
    }
    return map;
  }, [merged, needsIndex, headItems, index.data]);

  const complete = merged !== null || !needsIndex || index.data !== undefined;
  const totalBytes = useMemo(() => {
    if (!complete) return null;
    let sum = 0;
    for (const value of bytes.values()) sum += value;
    return sum;
  }, [bytes, complete]);

  const [seen] = useState(() => new Map<string, SeenTitle>());
  useEffect(() => {
    const visit = (row: LibraryRow) =>
      seen.set(row.card.id, {
        title: cardTitle(row.card),
        folder: row.path,
        filesMissing: problemOf(row.card) === "missing",
      });
    for (const row of merged ?? []) visit(row);
    for (const row of headItems ?? []) visit(row);
    for (const items of pageItems) for (const row of items ?? []) visit(row);
  });

  const moveRange = useCallback(
    (next: IndexRange) =>
      setRange((previous) =>
        previous.start === next.start && previous.end === next.end ? previous : next,
      ),
    [],
  );

  const failed = head.error ?? rest.find((r) => r.error !== null)?.error ?? index.error ?? null;
  return {
    total: listed,
    loading: windowed ? head.isPending : unionQueries.some((result) => result.isPending),
    error: failed,
    rowAt,
    keys,
    version: arrivals,
    seen,
    setRange: moveRange,
    bytesOf: (id) => bytes.get(id),
    totalBytes,
  };
}
