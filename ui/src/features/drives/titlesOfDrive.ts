import { useQuery } from "@tanstack/react-query";

import type { TableSort } from "../../components/Table";
import { currentLibraryBackend } from "../library/libraryBackend";
import type { LibraryQuery, LibraryRow } from "../library/libraryBackend";
import { cardTitle, problemOf, qualityTier } from "../library/titleFacts";
import type { Drive, DriveFolder } from "./drivesModel";

/** The last part of a path, whichever separator it uses. */
function lastSegment(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** One title of a drive as the table lists it. */
export interface DriveTitle {
  id: string;
  kind: "movie" | "show";
  title: string;
  year: number;
  /** The resolution class (`2160p`), or null when media info has not been read. */
  quality: string | null;
  hdr: string | null;
  /** The height of the video in lines, for sorting by quality. */
  height: number;
  bytes: number;
  /** The title's own folder, in full. */
  path: string;
  /** The name of that folder ("Heat (1995)"), what the Folder column shows. */
  folder: string;
  /** The title folder's label in the library ("Movies"). */
  source: string;
  filesMissing: boolean;
}

export type DriveSortColumn = "title" | "year" | "quality" | "size" | "folder";

/** The sort a drive opens with: the biggest titles first (spec `drives` "Titles of a drive"). */
export const BIGGEST_FIRST: TableSort = { column: "size", direction: "desc" };

const PAGE = 500;

/** The query for the titles of one library folder: its type, filtered by the folder's label. */
function folderQuery(folder: Pick<DriveFolder, "type" | "label">): LibraryQuery {
  return {
    kind: folder.type === "movie" ? "movies" : "shows",
    sort: [{ field: "title", descending: false }],
    search: "",
    filters: [
      { field: "library_source", negated: false, values: [folder.label], min: null, max: null },
    ],
  };
}

/** A library row as the table shows it. */
function toDriveTitle(row: LibraryRow, kind: "movie" | "show", preferOriginal = false): DriveTitle {
  return {
    id: row.card.id,
    kind,
    title: cardTitle(row.card, preferOriginal),
    year: row.card.year,
    quality: qualityTier(row.card.resolution),
    hdr: row.card.hdr,
    height: row.height,
    bytes: row.total_file_size ?? row.file_size ?? 0,
    path: row.path,
    folder: lastSegment(row.path),
    source: row.source_label,
    filesMissing: problemOf(row.card) === "missing",
  };
}

/** Every row of a query, read in pages. */
async function readAll(query: LibraryQuery): Promise<LibraryRow[]> {
  const backend = currentLibraryBackend();
  const first = await backend.rows(query, 0, PAGE);
  const rows = [...first.items];
  for (let offset = rows.length; offset < first.total; offset += PAGE) {
    const next = await backend.rows(query, offset, PAGE);
    if (next.items.length === 0) break;
    rows.push(...next.items);
  }
  return rows;
}

const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });

/** The titles in the order of `sort`; ties fall back to the title so the list never wobbles. */
export function sortDriveTitles(titles: readonly DriveTitle[], sort: TableSort): DriveTitle[] {
  const sign = sort.direction === "asc" ? 1 : -1;
  const by = (a: DriveTitle, b: DriveTitle): number => {
    switch (sort.column as DriveSortColumn) {
      case "year":
        return a.year - b.year;
      case "quality":
        return a.height - b.height;
      case "size":
        return a.bytes - b.bytes;
      case "folder":
        return collator.compare(a.folder, b.folder);
      default:
        return collator.compare(a.title, b.title);
    }
  };
  return [...titles].sort((a, b) => sign * by(a, b) || collator.compare(a.title, b.title));
}

const DRIVE_TITLES_KEY = ["drives", "titles"] as const;

/**
 * Every title of a drive's library folders (the library query, filtered by folder label), read
 * whole so the table can sort by any column, size included. Read again when the library changes.
 */
export function useDriveTitles(drive: Drive, preferOriginal: boolean) {
  const folders = drive.folders;
  const key = folders.map((folder) => `${folder.type}:${folder.label}`).join("|");
  return useQuery({
    queryKey: ["library", ...DRIVE_TITLES_KEY, drive.id, key, preferOriginal] as const,
    queryFn: async (): Promise<DriveTitle[]> => {
      const lists = await Promise.all(
        folders.map(async (folder) => {
          const kind = folder.type === "movie" ? "movie" : "show";
          const rows = await readAll(folderQuery(folder));
          return rows.map((row) => toDriveTitle(row, kind, preferOriginal));
        }),
      );
      return lists.flat();
    },
    enabled: folders.length > 0,
    staleTime: 30_000,
  });
}
