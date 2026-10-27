import { isTauri } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { commands, events } from "../../bindings";
import type { DrivesOverviewDto } from "../../bindings";
import type { DrivesOverview, EmptyResult, TrashContents } from "./drivesModel";
import { syntheticDrives, syntheticTrash } from "./syntheticDrives";

/** Where the drives come from: the drives service in the desktop app, a sample in a browser. */
export interface DrivesBackend {
  overview: () => Promise<DrivesOverview>;
  /** Calls `handler` when the numbers changed (a move, delete or scan ended, a drive came or went). */
  onChanged: (handler: () => void) => () => void;
  /** What Empty… would erase on the drive `driveId`, or on every drive (`null`). */
  trashContents: (driveId: string | null) => Promise<TrashContents>;
  /** Erases it; `confirm` is the word the person typed (the backend checks it again). */
  emptyTrash: (driveId: string | null, confirm: string) => Promise<EmptyResult>;
}

/** The overview as the pages read it: a number the wire could not carry is zero. */
function overviewOf(dto: DrivesOverviewDto): DrivesOverview {
  return {
    updatedMs: dto.updatedMs ?? 0,
    drives: dto.drives.map((drive) => ({
      ...drive,
      totalBytes: drive.totalBytes ?? 0,
      freeBytes: drive.freeBytes ?? 0,
      trashBytes: drive.trashBytes ?? 0,
      binBytes: drive.binBytes ?? 0,
      customTrash:
        drive.customTrash === null
          ? null
          : { path: drive.customTrash.path, bytes: drive.customTrash.bytes ?? 0 },
      notInLibraryBytes: drive.notInLibraryBytes,
      lastSeenMs: drive.lastSeenMs,
      folders: drive.folders.map((folder) => ({ ...folder, bytes: folder.bytes ?? 0 })),
    })),
  };
}

/** The desktop app: the drives service reads the disks; `drives-changed` says the numbers moved. */
export const tauriDrivesBackend: DrivesBackend = {
  overview: async () => {
    const result = await commands.drivesOverview();
    if (result.status === "error") throw new Error(result.error.message);
    return overviewOf(result.data);
  },
  onChanged: (handler) => {
    let stop: (() => void) | null = null;
    let closed = false;
    void events.drivesChanged
      .listen(() => handler())
      .then((unlisten) => {
        if (closed) unlisten();
        else stop = unlisten;
      });
    return () => {
      closed = true;
      stop?.();
    };
  },
  trashContents: async (driveId) => {
    const result = await commands.drivesTrashContents(driveId);
    if (result.status === "error") throw new Error(result.error.message);
    return {
      bytes: result.data.bytes ?? 0,
      entries: result.data.entries.map((entry) => ({
        kind: entry.kind === "bin" ? "bin" : "folder",
        path: entry.path,
        bytes: entry.bytes ?? 0,
      })),
    };
  },
  emptyTrash: async (driveId, confirm) => {
    const result = await commands.emptyDrivesTrash(driveId, confirm);
    if (result.status === "error") throw new Error(result.error.message);
    return { bytes: result.data.bytes ?? 0, problems: result.data.problems };
  },
};

let synthetic: DrivesBackend | null = null;
let override: DrivesBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setDrivesBackend(backend: DrivesBackend | null): void {
  override = backend;
}

/** The sample's trash, emptied for the rest of the session once Empty… ran. */
let syntheticEmptied = new Set<string>();

/** The sample with what Empty… already erased taken out. */
function syntheticOverview(): DrivesOverview {
  const overview = syntheticDrives();
  return {
    ...overview,
    drives: overview.drives.map((drive) =>
      syntheticEmptied.has(drive.id)
        ? {
            ...drive,
            trashBytes: 0,
            binBytes: 0,
            customTrash:
              drive.customTrash === null ? null : { path: drive.customTrash.path, bytes: 0 },
          }
        : drive,
    ),
  };
}

export function currentDrivesBackend(): DrivesBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriDrivesBackend;
  synthetic ??= {
    overview: () => Promise.resolve(syntheticOverview()),
    onChanged: () => () => undefined,
    trashContents: (driveId) => {
      const drives = syntheticOverview().drives.filter(
        (drive) => drive.online && (driveId === null || drive.id === driveId),
      );
      const entries = drives.flatMap((drive) => syntheticTrash(drive));
      return Promise.resolve({
        entries,
        bytes: entries.reduce((sum, entry) => sum + entry.bytes, 0),
      });
    },
    emptyTrash: (driveId, confirm) => {
      if (confirm !== "DELETE") return Promise.reject(new Error("Type DELETE to empty the trash."));
      const drives = syntheticOverview().drives.filter(
        (drive) => driveId === null || drive.id === driveId,
      );
      const bytes = drives.reduce(
        (sum, drive) => sum + syntheticTrash(drive).reduce((s, e) => s + e.bytes, 0),
        0,
      );
      syntheticEmptied = new Set([...syntheticEmptied, ...drives.map((drive) => drive.id)]);
      return Promise.resolve({ bytes, problems: [] });
    },
  };
  return synthetic;
}

export const DRIVES_KEY = ["drives", "overview"] as const;

/** Space numbers are read again at least this often while the app is in front. */
const REFRESH_MS = 60_000;

/**
 * The drives overview (spec `drives` "Space numbers stay current"): read when the page opens,
 * after the service says the numbers changed, when the window comes back to the front, and every
 * minute while it is in front (the interval pauses while the page is hidden).
 */
export function useDrivesOverview() {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: DRIVES_KEY,
    queryFn: () => currentDrivesBackend().overview(),
    staleTime: 15_000,
    refetchInterval: REFRESH_MS,
    refetchOnWindowFocus: true,
  });
  useEffect(
    () =>
      currentDrivesBackend().onChanged(
        () => void client.invalidateQueries({ queryKey: DRIVES_KEY }),
      ),
    [client],
  );
  return query;
}
