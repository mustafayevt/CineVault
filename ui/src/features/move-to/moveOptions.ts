import type { MovePlan } from "../library-ops/libraryOpsBackend";
import { moveBlocker, sourceName } from "../library-ops/moveModel";
import type { LibrarySource } from "../library-sources/sourcesBackend";
import { t } from "../../i18n/catalog";
import type { Drive } from "../drives/drivesModel";

/** One destination of the Move to menu with the numbers the menu shows. */
export interface MoveOption {
  source: LibrarySource;
  /** "E: Archive" when the drive is known, else the folder's label. */
  name: string;
  /** The folder's own label ("Movies 4K") when it says more than the drive's name, else null. */
  folder: string | null;
  path: string;
  /** The titles of the request this destination would take. */
  readyIds: string[];
  /** Bytes that would be copied there (a move inside one volume copies nothing). */
  movedBytes: number;
  /** Free space on the destination now, when the system says. */
  freeNow: number | null;
  /** Free space on the destination after the move. */
  freeAfter: number | null;
  /** The share of the destination drive in use after the move, when its size is known. */
  usedAfter: number | null;
  offline: boolean;
  /** Why the destination cannot be chosen; null when it can. */
  reason: string | null;
}

export type SizeText = (bytes: number) => string;

/** The drive that holds a library folder, by its label and type. */
function driveOfFolder(
  drives: readonly Drive[],
  label: string,
  type: "movie" | "tv",
): Drive | undefined {
  return drives.find((drive) =>
    drive.folders.some(
      (folder) => (folder.label === label || folder.path === label) && folder.type === type,
    ),
  );
}

/** The drive a library folder is on: by its label, else by the path starting at the drive. */
export function driveOfDestination(
  drives: readonly Drive[],
  source: Pick<LibrarySource, "label" | "path" | "kind">,
): Drive | undefined {
  return (
    driveOfFolder(drives, source.label, source.kind === "movie" ? "movie" : "tv") ??
    drives.find((drive) => source.path.toLowerCase().startsWith(drive.id.toLowerCase()))
  );
}

/**
 * Why none of the titles can go to a destination, in the menu's words: the space shortfall as
 * "Not enough space: needs 71.4 GB, 1.02 GB free." (the row already names the drive), an
 * unreachable folder without its path (the row shows it), else the backend's reason.
 */
function blockedReason(
  blocked: MovePlan["rows"][number] | undefined,
  name: string,
  size: SizeText,
): string {
  if (blocked === undefined) return t("move.blocker.other");
  if (blocked.status === "insufficient_space") {
    return t("moveTo.noRoom", {
      needed: size(blocked.needed_bytes ?? blocked.bytes ?? 0),
      free: size(blocked.free_bytes ?? 0),
    });
  }
  if (blocked.status === "unavailable") return t("move.blocker.unavailable");
  return moveBlocker(blocked, name, size) ?? t("move.blocker.other");
}

/**
 * What choosing a destination would do, from the plan the backend made for it: the titles it takes,
 * the space before and after, and the reason it is disabled (offline, none of the titles can go
 * there, or not enough room for the ones that can). Spec `drives` "Move to with space before and
 * after".
 */
export function buildOption(
  source: LibrarySource,
  plan: MovePlan,
  drive: Drive | undefined,
  size: SizeText,
): MoveOption {
  const name = drive?.name ?? sourceName(source);
  const ready = plan.rows.filter((row) => row.status === "ready");
  const copied = ready.filter((row) => row.across_volumes);
  const movedBytes = copied.reduce((sum, row) => sum + (row.bytes ?? 0), 0);
  const needed = copied.reduce((sum, row) => sum + (row.needed_bytes ?? row.bytes ?? 0), 0);
  const freeNow =
    plan.rows.find((row) => row.free_bytes !== null)?.free_bytes ?? drive?.freeBytes ?? null;
  const offline = drive !== undefined && !drive.online;
  const blocked = plan.rows.find((row) => row.status !== "ready");

  let reason: string | null = null;
  if (offline) reason = t("moveTo.offlineReason");
  else if (ready.length === 0) reason = blockedReason(blocked, name, size);
  else if (freeNow !== null && needed > freeNow) {
    reason = t("moveTo.noRoom", { needed: size(needed), free: size(freeNow) });
  }

  const freeAfter = freeNow === null ? null : Math.max(0, freeNow - movedBytes);
  const usedAfter =
    drive !== undefined && drive.totalBytes > 0 && freeAfter !== null
      ? 1 - freeAfter / drive.totalBytes
      : null;
  const label = sourceName(source);
  return {
    source,
    name,
    folder: label !== name && label !== source.path ? label : null,
    path: source.path,
    readyIds: ready.map((row) => row.id),
    movedBytes,
    freeNow,
    freeAfter,
    usedAfter,
    offline,
    reason,
  };
}

/** How much the source drive frees: its name and free space before and after the move. */
export interface SourceFrees {
  name: string;
  before: number;
  after: number;
}

/**
 * The drive the titles leave, when they all sit on one: it frees what is copied away. Null when
 * the titles come from several drives or the drive is not known.
 */
export function sourceFrees(
  plan: MovePlan,
  drives: readonly Drive[],
  movedBytes: number,
  type: "movie" | "tv",
): SourceFrees | null {
  const labels = new Set(plan.rows.map((row) => row.source));
  if (labels.size !== 1 || movedBytes <= 0) return null;
  const [label] = labels;
  const drive = label === undefined ? undefined : driveOfFolder(drives, label, type);
  return drive === undefined
    ? null
    : { name: drive.name, before: drive.freeBytes, after: drive.freeBytes + movedBytes };
}
