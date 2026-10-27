import { useQuery } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";

import { t } from "../../i18n/catalog";
import { currentHistoryBackend } from "../history/historyBackend";
import { undoHistoryEntry } from "../history/historyUndo";

/** The name a title had before CineVault renamed it, and when that happened. */
export interface RenamedFrom {
  /** The old folder name. */
  from: string;
  /** Epoch milliseconds of the rename. */
  atMs: number;
}

/**
 * Where the "Renamed from … · Undo" line of a title page gets its data: the History entries of
 * the title (spec `operation-history` "Renamed from line on title pages"). The line reads the
 * newest change of the title and shows only when that is a rename that still stands.
 */
export interface RenameHistoryBackend {
  renamedFrom: (id: string) => Promise<RenamedFrom | null>;
  /** Undoes that title's part of its last rename batch only. */
  undo: (id: string) => Promise<void>;
}

/** The last part of a folder name or path: "Dune.Part.Two.2024" for "D:\Movies\Dune.Part.Two.2024". */
function lastPart(path: string): string {
  return (
    path
      .split(/[\\/]/)
      .filter((part) => part !== "")
      .at(-1) ?? path
  );
}

/** The newest entry of the title when it is a rename that has not been taken back. */
async function lastRename(id: string) {
  const entries = await currentHistoryBackend().forTitle(id);
  const newest = entries.find((entry) => entry.preview.some((title) => title.state === "done"));
  const title = newest?.preview.find((entry) => entry.state === "done");
  if (newest === undefined || title === undefined) return null;
  if (newest.kind !== "rename" || newest.undo_state === "undone") return null;
  return { entry: newest, title };
}

/** The desktop app: the History service says what was renamed and takes one title back. */
const live: RenameHistoryBackend = {
  renamedFrom: async (id) => {
    const found = await lastRename(id);
    if (found === null || found.title.before === "") return null;
    return { from: lastPart(found.title.before), atMs: found.entry.at ?? 0 };
  },
  undo: async (id) => {
    const found = await lastRename(id);
    if (found === null) throw new Error(t("movie.renamedFrom.gone"));
    const report = await undoHistoryEntry(found.entry.id, id, { announce: false });
    const left = report.left[0];
    if (report.undone.length === 0) throw new Error(left?.reason ?? t("movie.renamedFrom.gone"));
  },
};

/** A browser has no database: the first fixture movie carries a rename so the line can be seen. */
const fixture: RenameHistoryBackend = {
  renamedFrom: (id) =>
    Promise.resolve(
      id === "1"
        ? {
            from: "Dune.Part.Two.2024.2160p.UHD.BluRay.x265-HDR",
            atMs: Date.now() - 2 * 60 * 60 * 1000,
          }
        : null,
    ),
  undo: () => Promise.resolve(),
};

let override: RenameHistoryBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setRenameHistoryBackend(backend: RenameHistoryBackend | null): void {
  override = backend;
}

export function currentRenameHistoryBackend(): RenameHistoryBackend {
  if (override !== null) return override;
  return isTauri() ? live : fixture;
}

/** The rename a title page reports, `null` when it was never renamed by CineVault. */
export function useRenamedFrom(id: string) {
  return useQuery({
    queryKey: ["title-renamed-from", id],
    queryFn: () => currentRenameHistoryBackend().renamedFrom(id),
    staleTime: 0,
  });
}
