import { t } from "../../i18n/catalog";
import type { SourceKind } from "../library-sources/sourcesBackend";
import type { FolderAnalysis, FolderGuess, SuggestedKind } from "./onboardingBackend";

/** A folder on the first step: what was found inside, and the library it will be added to. */
export interface FolderEntry {
  path: string;
  status: "looking" | "ready" | "failed";
  /** What the folder holds; `null` while looking or when it could not be read. */
  found: {
    guess: FolderGuess;
    suggested: SuggestedKind;
    videos: number;
    truncated: boolean;
    /** The first video that is not an episode, for the Naming step's example. */
    firstVideo: string | null;
  } | null;
  /** The library it is added to. Starts as the detected type; the switch on the row changes it. */
  kind: SourceKind;
  /** Why it cannot be added, in words. */
  error: string | null;
  /** Added to the library already (a later step failed and the user came back). */
  added: boolean;
}

/** A folder that is being looked into. */
export function lookingEntry(path: string): FolderEntry {
  return { path, status: "looking", found: null, kind: "movie", error: null, added: false };
}

/** A folder that could not be read. */
export function failedEntry(path: string, error: string): FolderEntry {
  return { ...lookingEntry(path), status: "failed", error };
}

/** A folder from the backend's analysis. A folder with both kinds starts as movies. */
export function readyEntry(analysis: FolderAnalysis): FolderEntry {
  return {
    path: analysis.path,
    status: "ready",
    found: {
      guess: analysis.guess,
      suggested: analysis.suggested,
      videos: analysis.videos,
      truncated: analysis.truncated,
      firstVideo: analysis.first_video,
    },
    kind: analysis.suggested === "tv" ? "tv" : "movie",
    error: null,
    added: false,
  };
}

/** A path compared without regard to case, separators or a trailing separator. */
function normalPath(path: string): string {
  return path
    .replace(/[\\/]+$/, "")
    .replaceAll("\\", "/")
    .toLowerCase();
}

/** Case-insensitive on Windows and macOS volumes; separators do not matter. */
export function samePath(a: string, b: string): boolean {
  return normalPath(a) === normalPath(b);
}

/** Whether `inner` lies below `outer` (not the same folder). */
function isInside(inner: string, outer: string): boolean {
  const base = normalPath(outer);
  return normalPath(inner).startsWith(base.endsWith("/") ? base : `${base}/`);
}

/** Another folder of the list, of the same type, that a folder lies inside or holds. */
export interface Overlap {
  other: string;
  /** The folder lies inside `other`; otherwise it holds it. */
  inside: boolean;
}

/**
 * The folders that overlap another folder of the same type in the list, as the library refuses
 * them: a folder inside another one of the same kind is covered by it already, and a folder that
 * holds one cannot be added while that one is. Movie and TV folders may lie inside each other;
 * each library leaves the other's folders alone.
 */
export function overlaps(entries: readonly FolderEntry[]): Map<string, Overlap> {
  const ready = entries.filter((entry) => entry.status === "ready");
  const found = new Map<string, Overlap>();
  for (const entry of ready) {
    for (const other of ready) {
      if (other === entry || other.kind !== entry.kind || found.has(entry.path)) continue;
      if (isInside(entry.path, other.path)) {
        found.set(entry.path, { other: other.path, inside: true });
      } else if (isInside(other.path, entry.path)) {
        found.set(entry.path, { other: other.path, inside: false });
      }
    }
  }
  return found;
}

/** The folders that will be added: the readable ones. */
export function foldersToAdd(entries: readonly FolderEntry[]): FolderEntry[] {
  return entries.filter((entry) => entry.status === "ready");
}

/**
 * Whether Continue can go on from the folders: something to add, nothing still being looked
 * into, and no two folders of the same type inside each other.
 */
export function readyToContinue(entries: readonly FolderEntry[]): boolean {
  return (
    foldersToAdd(entries).length > 0 &&
    entries.every((entry) => entry.status !== "looking") &&
    overlaps(entries).size === 0
  );
}

/** Why Continue waits on the folders step, in words. */
export function folderStepReason(entries: readonly FolderEntry[]): string {
  if (entries.some((entry) => entry.status === "looking")) return t("setup.folders.stillLooking");
  if (overlaps(entries).size > 0) return t("setup.folders.overlap");
  return t("setup.folders.needOne");
}
