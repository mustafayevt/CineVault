import type { ScoreTone } from "../../components/ScoreBadge";
import type { LibraryRow } from "../library/libraryBackend";
import { cardTitle, yearText } from "../library/titleFacts";
import type { ReviewCandidate, ReviewItem } from "./reviewBackend";
import type { ScrapeKind, ScrapeStreamHit } from "./scrapeBackend";
import { hitName } from "./scrapeModel";
import { subjectName } from "./titleSubject";
import type { TitleSubject } from "./titleSubject";

/**
 * A top candidate at or above this score, and not tied with the second, is confident (spec
 * `title-matching` "Confident means 90 percent and clear"). The rule is fixed: no setting. The
 * backend owns it (`cv_match::accept::CONFIDENT` and `is_confident`) and decides what Accept all
 * confident takes; this mirror only colours the scores and counts the review rows it lists.
 */
const CONFIDENT_PERCENT = 90;

/** The tabs of the queue. */
export type QueueTab = "all" | "new" | "uncertain" | "notMatched";

/**
 * Why a title waits for a decision. `scraped` is a title the user sent to the Scrape page from the
 * Library that already has a match: it is listed under All only.
 */
export type QueueStatus = "new" | "uncertain" | "notMatched" | "scraped";

/** One title of the queue. */
export interface QueueEntry {
  /** Unique in the queue and stable while the title waits. */
  key: string;
  /** The library item. */
  itemId: string;
  kind: ScrapeKind;
  status: QueueStatus;
  /** The title as the file names read it, with the year when there is one. */
  name: string;
  /** The title alone and its year, for the search field. */
  title: string;
  year: number | null;
  /** The file or folder name shown under the title. */
  file: string;
  /** The full path of the file or folder. */
  path: string;
  /** Size and runtime of the file when known, for the evidence line. */
  sizeBytes: number | null;
  runtimeSeconds: number | null;
  /** Quality such as "2160p", when known. */
  quality: string | null;
  /** The review row of a title already searched; `null` for a New title. */
  review: ReviewItem | null;
}

/** A confidence (0 to 1) as a whole percentage. */
export function percentOf(confidence: number | null): number {
  return Math.round((confidence ?? 0) * 100);
}

/** The tone of a score: confident, uncertain or unlikely. */
export function scoreTone(percent: number): ScoreTone {
  if (percent >= CONFIDENT_PERCENT) return "ok";
  if (percent >= 40) return "warn";
  return "bad";
}

/** The candidates of a review row as the entries of a search, so the page shows them at once. */
export function seedHits(item: ReviewItem): ScrapeStreamHit[] {
  return item.candidates.map((candidate: ReviewCandidate) => ({
    id: `seed:${candidate.provider}:${candidate.providerId}`,
    candidate,
    providers: [
      {
        provider: candidate.provider,
        providerId: candidate.providerId,
        confidence: candidate.confidence,
      },
    ],
    ids: [],
    inLibrary: null,
    absorbs: [],
  }));
}

/** Two candidates with the same whole percentage at the top: the user has to choose. */
function isTied(hits: readonly ScrapeStreamHit[], flagged = false): boolean {
  if (flagged) return true;
  const [first, second] = hits;
  if (first === undefined || second === undefined) return false;
  return percentOf(first.candidate.confidence) === percentOf(second.candidate.confidence);
}

/** The top candidate scores 90% or more and is not tied with the second. */
export function isConfident(hits: readonly ScrapeStreamHit[], flagged = false): boolean {
  const top = hits[0];
  if (top === undefined) return false;
  // The unrounded score, as the backend reads it: 89.6% shows as 90% but is not confident.
  return (
    (top.candidate.confidence ?? 0) >= CONFIDENT_PERCENT / 100 - 1e-9 && !isTied(hits, flagged)
  );
}

/** The queue entry of a review row: Uncertain with candidates, Not matched without. */
export function entryOfReview(item: ReviewItem): QueueEntry {
  const { local } = item;
  const year = local.year;
  const title = local.title === "" ? item.name : local.title;
  return {
    key: `review:${item.id}`,
    itemId: item.itemId,
    kind: item.kind,
    status: item.candidates.length === 0 ? "notMatched" : "uncertain",
    name: year === null ? title : `${title} (${year})`,
    title,
    year,
    file: fileNameOf(item.path) || item.name,
    path: item.path,
    sizeBytes: null,
    runtimeSeconds: local.runtimeSeconds,
    quality: null,
    review: item,
  };
}

/** The queue entry of a New title, from its library row. */
export function entryOfNew(row: LibraryRow, kind: ScrapeKind, key: string): QueueEntry {
  const title = cardTitle(row.card);
  const year = row.card.year > 0 ? row.card.year : null;
  return {
    key,
    itemId: row.card.id,
    kind,
    status: "new",
    name: year === null ? title : `${title} (${yearText(year)})`,
    title,
    year,
    file: row.filename !== "" ? row.filename : row.card.file_name,
    path: row.path,
    sizeBytes: row.total_file_size ?? row.file_size,
    runtimeSeconds: row.runtime_minutes > 0 ? row.runtime_minutes * 60 : null,
    quality: row.card.resolution,
    review: null,
  };
}

/** The queue entry of a title sent from the Library: its review row when it has one. */
export function entryOfSubject(subject: TitleSubject, review: ReviewItem | null): QueueEntry {
  if (review !== null) return entryOfReview(review);
  const year = subject.year > 0 ? subject.year : null;
  return {
    key: `title:${subject.id}`,
    itemId: subject.id,
    kind: subject.kind,
    status: subject.lifecycle?.stage === "new" ? "new" : "scraped",
    name: subjectName(subject),
    title: subject.title,
    year,
    file: subject.fileName !== "" ? subject.fileName : fileNameOf(subject.folder),
    path: subject.folder,
    sizeBytes: subject.fileBytes,
    runtimeSeconds: subject.durationSeconds,
    quality: null,
    review: null,
  };
}

/** The last part of a path, whatever its separators. */
function fileNameOf(path: string): string {
  return (
    path
      .split(/[\\/]/)
      .filter((part) => part !== "")
      .pop() ?? path
  );
}

/** "Solaris (1972) or Solaris (2002)?" for a close call between two. */
export function eitherName(hits: readonly ScrapeStreamHit[]): string | null {
  const [first, second] = hits;
  if (first === undefined || second === undefined) return null;
  return `${hitName(first.candidate)} or ${hitName(second.candidate)}`;
}

/** Whether a tab lists an entry. */
export function inTab(entry: Pick<QueueEntry, "status">, tab: QueueTab): boolean {
  return tab === "all" || entry.status === tab;
}
