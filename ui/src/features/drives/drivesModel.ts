/**
 * The drives of the library as the Drives page and the Move to menu read them. The
 * shape is the one the drives service returns from `drives_overview`; until that is
 * wired the page reads it through `drivesBackend.ts`.
 */

/** The kind of device a drive is. */
export type DriveKind = "hdd" | "ssd" | "network" | "removable";

/** One library folder on a drive. */
export interface DriveFolder {
  /** The data source id; `plan_change_source` takes it as the move target. */
  sourceId: number;
  path: string;
  /** The folder's label in the Library's "Drive" filter ("Movies", "NAS 4K"). */
  label: string;
  type: "movie" | "tv";
  /** Titles in it (shows for a TV folder). */
  titles: number;
  bytes: number;
}

/** How many titles of a drive fall in each resolution class. */
export interface QualityCounts {
  p2160: number;
  p1080: number;
  p720: number;
  sd: number;
}

export interface Drive {
  /** Stable id: the drive letter ("D:"), a share name or a mount point. */
  id: string;
  /** "D: Media": the letter or mount, then the volume label. */
  name: string;
  kind: DriveKind;
  totalBytes: number;
  freeBytes: number;
  folders: readonly DriveFolder[];
  /** Used space that is not library titles; null when under 1 GB or unknown. */
  notInLibraryBytes: number | null;
  quality: QualityCounts;
  online: boolean;
  /** When an offline drive was last reachable, epoch milliseconds. */
  lastSeenMs: number | null;
  /** The low-space rule of spec `drives`, decided by the backend. */
  low: boolean;
  /** Bytes in the trash folders (`.deletedByCineVault`) of its library folders. */
  trashBytes: number;
  /** Bytes CineVault itself sent to the Recycle Bin from this drive that are still there. */
  binBytes: number;
  /** The custom trash folder of Settings › Deleted files when it is on this drive. */
  customTrash: { path: string; bytes: number } | null;
}

/** One entry of what Empty… would erase. */
export interface TrashEntry {
  /** A batch of a trash folder, or a path CineVault sent to the Recycle Bin. */
  kind: "folder" | "bin";
  path: string;
  bytes: number;
}

/** What Empty… would erase on one drive, or on all of them. */
export interface TrashContents {
  entries: readonly TrashEntry[];
  bytes: number;
}

/** What an Empty did. */
export interface EmptyResult {
  bytes: number;
  problems: readonly string[];
}

/** The bytes Empty… would free on a drive: its trash folders, the custom one, its bin items. */
export function trashTotal(drive: Pick<Drive, "trashBytes" | "binBytes" | "customTrash">): number {
  return drive.trashBytes + drive.binBytes + (drive.customTrash?.bytes ?? 0);
}

export interface DrivesOverview {
  drives: readonly Drive[];
  /** When the numbers were read, epoch milliseconds. */
  updatedMs: number;
}

export const NO_DRIVES: DrivesOverview = { drives: [], updatedMs: 0 };

/** "D:" and "Media" from "D: Media"; a name without a leading letter or mount has no head. */
export function nameParts(name: string): { head: string; rest: string } {
  const match = /^(\S+:)\s+(.*)$/.exec(name) ?? /^([A-Za-z]:)()$/.exec(name);
  return match === null ? { head: "", rest: name } : { head: match[1] ?? "", rest: match[2] ?? "" };
}

/** The share of the drive in use, 0 to 1. */
export function usedShare(drive: Pick<Drive, "totalBytes" | "freeBytes">): number {
  return drive.totalBytes > 0 ? 1 - drive.freeBytes / drive.totalBytes : 0;
}

/** How many drives are offline. */
export function offlineCount(drives: readonly Drive[]): number {
  return drives.filter((drive) => !drive.online).length;
}

export interface QualityShare {
  id: "2160p" | "1080p" | "720p" | "SD";
  count: number;
  /** Width of its segment in the breakdown bar, 0 to 100. */
  percent: number;
}

/** The non-empty resolution classes with their share of the drive's titles, best first. */
export function qualityShares(quality: QualityCounts): QualityShare[] {
  const all: [QualityShare["id"], number][] = [
    ["2160p", quality.p2160],
    ["1080p", quality.p1080],
    ["720p", quality.p720],
    ["SD", quality.sd],
  ];
  const total = all.reduce((sum, [, count]) => sum + count, 0);
  return all
    .filter(([, count]) => count > 0)
    .map(([id, count]) => ({ id, count, percent: total === 0 ? 0 : (count / total) * 100 }));
}

/** The words of a past moment: "just now", "2 days ago". `now` is passed in for tests. */
export function agoText(ms: number, now: number, locale: string): string {
  const seconds = Math.max(0, Math.round((now - ms) / 1000));
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86_400],
    ["hour", 3600],
    ["minute", 60],
  ];
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: "always" });
  for (const [unit, size] of units) {
    if (seconds >= size) return formatter.format(-Math.floor(seconds / size), unit);
  }
  return "";
}
