import type { MoveProgressDto, MoveRowDto } from "../../bindings";
import { t } from "../../i18n/catalog";
import type { LibrarySource } from "../library-sources/sourcesBackend";

/** Why a destination cannot be picked at all: it is where the title is now, or the wrong kind. */
export type DestinationBlock = "same" | "kind" | null;

/** The word for one folder of the library: its label, or its path when it has none. */
export function sourceName(source: Pick<LibrarySource, "label" | "path">): string {
  return source.label === "" ? source.path : source.label;
}

/**
 * Why the move cannot start, in a sentence the user can act on, or `null` when it can. The size
 * formatter writes the free-space figures in the user's units.
 */
export function moveBlocker(
  row: MoveRowDto,
  target: string,
  size: (bytes: number) => string,
): string | null {
  switch (row.status) {
    case "ready":
      return null;
    case "same_source":
      return t("move.blocker.same", { target });
    case "wrong_kind":
      return t(row.kind === "show" ? "move.blocker.kindShow" : "move.blocker.kindMovie");
    case "shared_folder":
      return t("move.blocker.shared");
    case "in_source_root":
      return t("move.blocker.root");
    case "unavailable":
      return row.reason === "" ? t("move.blocker.unavailable") : row.reason;
    case "destination_taken":
      return t("move.blocker.taken", { target });
    case "insufficient_space":
      return t("move.blocker.space", {
        needed: size(row.needed_bytes ?? row.bytes ?? 0),
        free: size(row.free_bytes ?? 0),
        target,
      });
    default:
      return row.reason === "" ? t("move.blocker.other") : row.reason;
  }
}

/** "4 min left", "1 h 5 min left", "Under a minute left". */
export function timeLeftText(seconds: number): string {
  if (seconds < 60) return t("move.progress.left.underMinute");
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return t("move.progress.left.minutes", { count: minutes });
  return t("move.progress.left.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}

/**
 * The speed of a copy and the time left, "142 MB/s · 4 min left", or `null` before the backend
 * knows them (a rename on one drive copies nothing). `size` writes bytes in the user's units.
 */
export function moveSpeedText(
  progress: Pick<MoveProgressDto, "bytes_per_second" | "seconds_left"> | null,
  size: (bytes: number) => string,
): string | null {
  const rate = progress?.bytes_per_second ?? 0;
  if (progress === null || rate <= 0) return null;
  const speed = t("move.progress.speed", { size: size(rate) });
  const left = progress.seconds_left;
  return left === null || left === undefined ? speed : `${speed} · ${timeLeftText(left)}`;
}

/**
 * How far a move is, 0 to 100: by bytes while files are copied, by titles for a rename (which
 * copies nothing) and before anything has been reported.
 */
export function movePercent(progress: MoveProgressDto | null): number {
  if (progress === null) return 0;
  const total = progress.bytes_total ?? 0;
  if (total > 0) return Math.min(100, ((progress.bytes_done ?? 0) / total) * 100);
  return progress.total === 0 ? 0 : Math.min(100, (progress.done / progress.total) * 100);
}
