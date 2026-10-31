import { useCallback, useState } from "react";

import type { TableSort } from "../../components/Table";
import type { QualityTier } from "../../components/QualityBadge";
import { t } from "../../i18n/catalog";
import type { SourceInfo, TorrentRelease } from "./torrentBackend";
import { resolutionOf, siteName, sourceName } from "./torrentModel";

/** The columns the release table sorts by (spec `torrent-search` "Release table columns and sort"). */
export type ReleaseSortColumn = "size" | "seeders" | "source" | "age" | "quality";

/** Most seeders first. */
export const DEFAULT_SORT: TableSort = { column: "seeders", direction: "desc" };

const COLUMNS: readonly string[] = ["size", "seeders", "source", "age"];
const STORAGE_KEY = "cinevault.torrents.sort";

/** The feed's default: the newest first (the smallest age). */
export const FEED_DEFAULT_SORT: TableSort = { column: "age", direction: "asc" };
const FEED_COLUMNS: readonly string[] = ["size", "seeders", "source", "age", "quality"];
const FEED_STORAGE_KEY = "cinevault.torrents.latestSort";

/** The sort the user chose last, or the default when none was saved or it is damaged. */
export function readSort(
  key: string = STORAGE_KEY,
  columns: readonly string[] = COLUMNS,
  fallback: TableSort = DEFAULT_SORT,
): TableSort {
  try {
    const raw = window.localStorage.getItem(key);
    if (raw === null) return fallback;
    const value = JSON.parse(raw) as Partial<TableSort> | null;
    if (
      value !== null &&
      typeof value.column === "string" &&
      columns.includes(value.column) &&
      (value.direction === "asc" || value.direction === "desc")
    ) {
      return { column: value.column, direction: value.direction };
    }
  } catch {
    // A damaged or unreadable entry means the default.
  }
  return fallback;
}

/** Saves the sort; storage that is full or switched off only loses the preference. */
function writeSort(sort: TableSort, key: string = STORAGE_KEY): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(sort));
  } catch {
    // The sort stays in memory for this session.
  }
}

/** The sort of the release table, remembered across restarts. */
export function useRememberedSort(): [TableSort, (sort: TableSort) => void] {
  const [sort, setSort] = useState<TableSort>(() => readSort());
  const change = useCallback((next: TableSort) => {
    setSort(next);
    writeSort(next);
  }, []);
  return [sort, change];
}

/** The sort of the "Latest releases" table (newest first by default), remembered on its own. */
export function useFeedSort(): [TableSort, (sort: TableSort) => void] {
  const [sort, setSort] = useState<TableSort>(() =>
    readSort(FEED_STORAGE_KEY, FEED_COLUMNS, FEED_DEFAULT_SORT),
  );
  const change = useCallback((next: TableSort) => {
    setSort(next);
    writeSort(next, FEED_STORAGE_KEY);
  }, []);
  return [sort, change];
}

const RESOLUTION_RANK: Readonly<Record<string, number>> = {
  "2160p": 4,
  "1080p": 3,
  "720p": 2,
  "480p": 1,
};
const SOURCE_RANK: Readonly<Record<string, number>> = {
  Remux: 6,
  BluRay: 5,
  "WEB-DL": 4,
  WEBRip: 3,
  HDTV: 2,
  DVD: 1,
};

/**
 * How good a release looks by its name, higher is better: the resolution, then HDR, then the
 * source (remux, disc, web download, web rip, TV, DVD). A cam or screener counts as nothing.
 */
export function qualityRank(
  release: Pick<TorrentRelease, "resolution" | "hdr" | "source" | "flags">,
): number {
  const low = release.flags.some((flag) =>
    ["cam", "telesync", "telecine", "screener"].includes(flag.kind),
  );
  if (low) return 0;
  const resolution = RESOLUTION_RANK[release.resolution] ?? 0;
  const hdr = release.hdr === null ? 0 : 1;
  const source = release.source === null ? 0 : (SOURCE_RANK[release.source] ?? 0);
  return resolution * 100 + hdr * 10 + source;
}

/** The resolution chip a release belongs to: 480p and unknown are SD. */
export function tierOf(release: Pick<TorrentRelease, "resolution">): QualityTier {
  const resolution = resolutionOf(release);
  return resolution === "2160p" || resolution === "1080p" || resolution === "720p"
    ? resolution
    : "SD";
}

export const TIERS: readonly QualityTier[] = ["2160p", "1080p", "720p", "SD"];

/** How many releases each resolution chip holds. */
export function tierCounts(releases: readonly TorrentRelease[]): Record<QualityTier, number> {
  const counts: Record<QualityTier, number> = { "2160p": 0, "1080p": 0, "720p": 0, SD: 0 };
  for (const release of releases) counts[tierOf(release)] += 1;
  return counts;
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/**
 * The releases in the order of `sort`. A release without the figure (no seeders count, no date)
 * goes last whichever way the column sorts; ties fall back to the name so the table never wobbles
 * as releases arrive. `sourceOf` gives the text the Source column shows.
 */
export function sortReleases(
  releases: readonly TorrentRelease[],
  sort: TableSort,
  sourceOf: (release: TorrentRelease) => string,
): TorrentRelease[] {
  const sign = sort.direction === "asc" ? 1 : -1;
  const compare = (a: TorrentRelease, b: TorrentRelease): number => {
    let pair: [number | null, number | null];
    switch (sort.column as ReleaseSortColumn) {
      case "size":
        pair = [a.size, b.size];
        break;
      case "age":
        // The column is the age: a newer release is the smaller one.
        pair = [
          a.published === null ? null : -a.published,
          b.published === null ? null : -b.published,
        ];
        break;
      case "quality":
        pair = [qualityRank(a), qualityRank(b)];
        break;
      case "source":
        return (
          sign * collator.compare(sourceOf(a), sourceOf(b)) || collator.compare(a.name, b.name)
        );
      default:
        pair = [a.seeders, b.seeders];
    }
    const [left, right] = pair;
    // Unknown figures last, in either direction.
    if (left === null || right === null) {
      if (left === right) return collator.compare(a.name, b.name);
      return left === null ? 1 : -1;
    }
    return sign * (left - right) || collator.compare(a.name, b.name);
  };
  return [...releases].sort(compare);
}

/** The sources of a release as its row shows them: the first one, and how many more. */
export function sourceText(
  release: TorrentRelease,
  sources: readonly Pick<SourceInfo, "id" | "name">[],
): string {
  const names = release.sources.map((id) => siteName(sourceName(id, sources)));
  const [first] = names;
  if (first === undefined) return "";
  return names.length > 1
    ? t("torrentsPage.source.more", { name: first, count: names.length - 1 })
    : first;
}
