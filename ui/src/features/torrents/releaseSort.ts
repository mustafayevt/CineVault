import { useCallback, useState } from "react";

import type { TableSort } from "../../components/Table";
import type { QualityTier } from "../../components/QualityBadge";
import { t } from "../../i18n/catalog";
import type { SourceInfo, TorrentRelease } from "./torrentBackend";
import { resolutionOf, siteName, sourceName } from "./torrentModel";

/** The columns the release table sorts by (spec `torrent-search` "Release table columns and sort"). */
export type ReleaseSortColumn = "size" | "seeders" | "source" | "age";

/** Most seeders first. */
export const DEFAULT_SORT: TableSort = { column: "seeders", direction: "desc" };

const COLUMNS: readonly string[] = ["size", "seeders", "source", "age"];
const STORAGE_KEY = "cinevault.torrents.sort";

/** The sort the user chose last, or the default when none was saved or it is damaged. */
export function readSort(): TableSort {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === null) return DEFAULT_SORT;
    const value = JSON.parse(raw) as Partial<TableSort> | null;
    if (
      value !== null &&
      typeof value.column === "string" &&
      COLUMNS.includes(value.column) &&
      (value.direction === "asc" || value.direction === "desc")
    ) {
      return { column: value.column, direction: value.direction };
    }
  } catch {
    // A damaged or unreadable entry means the default.
  }
  return DEFAULT_SORT;
}

/** Saves the sort; storage that is full or switched off only loses the preference. */
function writeSort(sort: TableSort): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(sort));
  } catch {
    // The sort stays in memory for this session.
  }
}

/** The sort of the release table, remembered across restarts. */
export function useRememberedSort(): [TableSort, (sort: TableSort) => void] {
  const [sort, setSort] = useState<TableSort>(readSort);
  const change = useCallback((next: TableSort) => {
    setSort(next);
    writeSort(next);
  }, []);
  return [sort, change];
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
export function sourceText(release: TorrentRelease, sources: readonly SourceInfo[]): string {
  const names = release.sources.map((id) => siteName(sourceName(id, sources)));
  const [first] = names;
  if (first === undefined) return "";
  return names.length > 1
    ? t("torrentsPage.source.more", { name: first, count: names.length - 1 })
    : first;
}
