import type { QualityTier } from "../../components/QualityBadge";
import type { StageBadgeKind } from "../../components/StageBadge";
import { t } from "../../i18n/catalog";
import type { LibraryCard, LibraryRow } from "./libraryBackend";

/** The file name without its extension: the last resort for a title the parser could not read. */
function stem(fileName: string): string {
  return fileName.replace(/\.[A-Za-z0-9]{2,4}$/, "");
}

/** The best-known title of a card: the provider's, else the one parsed from the file name. */
export function cardTitle(card: LibraryCard, preferOriginal = false): string {
  if (card.unmatched) return card.title.trim() === "" ? stem(card.file_name) : card.title.trim();
  if (preferOriginal && card.original_title.trim() !== "") return card.original_title.trim();
  return card.title;
}

/** "2022", or "????" when the year is not known. */
export function yearText(year: number): string {
  return year > 0 ? String(year) : "????";
}

function hasFlag(card: LibraryCard, kind: string): boolean {
  return card.lifecycle?.flags.some((flag) => flag.kind === kind) ?? false;
}

/** The problem a title has, in the order the filters list them; null when it has none. */
export type TitleProblem = "not-matched" | "uncertain" | "missing";

export function problemOf(card: LibraryCard): TitleProblem | null {
  if (hasFlag(card, "not-matched")) return "not-matched";
  if (hasFlag(card, "needs-review") || (card.lifecycle === null && card.needs_review)) {
    return "uncertain";
  }
  if (hasFlag(card, "missing")) return "missing";
  return null;
}

/** The warning under a title that has a problem: "Not matched", "Uncertain", "Files missing". */
export function problemNote(card: LibraryCard): string | null {
  switch (problemOf(card)) {
    case "not-matched":
      return t("library.problem.notMatched");
    case "uncertain":
      return t("library.problem.uncertain");
    case "missing":
      return t("library.problem.missing");
    case null:
      return null;
  }
}

/** The badge on a poster: the stage, or the warning triangle when the title needs attention. */
export function stageBadgeOf(card: LibraryCard): StageBadgeKind {
  if (problemOf(card) !== null) return "attention";
  return card.lifecycle?.stage ?? "scraped";
}

/** The quality tier of a resolution label such as "2160p" or "4K"; null when unknown. */
export function qualityTier(resolution: string | null): QualityTier | null {
  if (resolution === null || resolution.trim() === "") return null;
  if (/4k|uhd/i.test(resolution)) return "2160p";
  const lines = Number.parseInt(resolution, 10);
  if (!Number.isFinite(lines)) return null;
  if (lines >= 1500) return "2160p";
  if (lines >= 900) return "1080p";
  if (lines >= 600) return "720p";
  return "SD";
}

/** The parent folder of a title's folder, which is the library folder the title sits in. */
export function libraryFolderOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut > 0 ? path.slice(0, cut) : path;
}

/** The rating text of a row: one decimal, or a dash. */
export function ratingText(row: LibraryRow): string {
  return row.rating === null ? "–" : row.rating.toFixed(1);
}

/** Runtime as "2h 28m". */
export function runtimeText(minutes: number): string {
  if (minutes <= 0) return "–";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours === 0 ? `${rest}m` : `${hours}h ${rest}m`;
}
