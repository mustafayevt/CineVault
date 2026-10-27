import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  EvidenceVerdictDto,
  MatchCommandError,
  MatchModeDto,
  ReviewCandidateDto,
  ReviewEvidenceTickDto,
  ReviewItemDto,
  ReviewLocalEvidenceDto,
  ReviewScoreBreakdownDto,
} from "../../bindings";
import { currentLifecycleBackend } from "../lifecycle/lifecycleBackend";
import { createSyntheticReviewBackend, syntheticReviewItems } from "./syntheticReview";

/** Whether a candidate agrees with what is known locally. `unknown` means nothing to compare. */
export type EvidenceVerdict = EvidenceVerdictDto;

/** One evidence check on a candidate card: a tick, a cross or a neutral dot, with its text. */
export type ReviewEvidenceTick = ReviewEvidenceTickDto;

/** Replaces the `null` the generated types use for a number that JSON cannot carry. */
type Numbers<T> = { [K in keyof T]: number };

/** How a confidence was reached, as stored with the review row. All values are fractions. */
export type ReviewScoreBreakdown = Numbers<ReviewScoreBreakdownDto>;

/**
 * A title the matcher found for a queued item. Accepting asks the candidate's `provider` for the
 * details. `posterKey` is a `cv://img` key; `null` shows a generated placeholder.
 */
export type ReviewCandidate = Omit<ReviewCandidateDto, "confidence" | "rank" | "breakdown"> & {
  /** Confidence, 0 to 1. */
  confidence: number;
  /** What lists are ordered by, best first: the confidence plus how well known the title is. */
  rank: number;
  breakdown: ReviewScoreBreakdown;
};

/** What the scan found about the item on disk. */
export type ReviewLocalEvidence = ReviewLocalEvidenceDto;

/** One entry of the Needs review inbox. */
export type ReviewItem = Omit<ReviewItemDto, "candidates"> & {
  /** Best first. */
  candidates: ReviewCandidate[];
};

/** The ways a match batch can run (see the Activity drawer for the jobs it starts). */
export type MatchBatchMode = MatchModeDto;

/** What a started batch reports. */
export interface MatchBatchResult {
  /** Titles that were queued. */
  planned: number;
  /** Titles no ready provider can serve. */
  withoutProvider: number;
}

/** A candidate to accept: provider and ID, so a manual search result works too. */
export interface ReviewChoice {
  provider: string;
  providerId: string;
}

/** A refusal from the backend: `message` is ready to show, `code` says why. */
export class ReviewRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ReviewRequestError";
    this.code = code;
  }
}

/** Where the inbox gets its queue and how decisions are stored. */
export interface ReviewBackend {
  /** False when nothing stores a review queue here, so the screen explains instead of listing. */
  readonly available: boolean;
  /** The pending items, in the order they are reviewed. Ignored items are never listed. */
  list: () => Promise<ReviewItem[]>;
  /** How many items are waiting in the queue itself (the sidebar reads the pipeline counts). */
  count: () => Promise<number>;
  /** Matches the item to the candidate and scrapes it. The item leaves the queue. */
  accept: (reviewId: number, choice: ReviewChoice) => Promise<void>;
  /** Accepts the top candidate of every item. Resolves to the review ids that were accepted. */
  acceptTop: (reviewIds: readonly number[]) => Promise<number[]>;
  /** Marks the item "not a catalogued title". It stays ignored: scans never queue it again. */
  ignore: (reviewId: number) => Promise<void>;
  /**
   * Puts accepted or ignored items back in the queue (the Undo of both). An item accepted in this
   * session is also put back as it was before the scrape; files already written stay.
   */
  reopen: (reviewIds: readonly number[]) => Promise<void>;
  /** Searches by title or by an ID shortcut for the item, with the usual scoring. */
  search: (reviewId: number, query: string) => Promise<ReviewCandidate[]>;
  /**
   * Starts matching library items as a background job; uncertain ones arrive in the inbox.
   * Resolves to how many were queued. Absent when the backend cannot start matching.
   */
  matchItems?: (itemIds: readonly string[]) => Promise<number>;
  /**
   * Starts a match batch in any mode: `match_unmatched` over the library (or the given titles),
   * `rescrape_by_ids` and `force_best_match` over the given titles. Absent when the backend
   * cannot start matching.
   */
  startBatch?: (
    mode: MatchBatchMode,
    itemIds: readonly string[] | null,
  ) => Promise<MatchBatchResult>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: MatchCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ReviewRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const orZero = (value: number | null): number => value ?? 0;

function candidateOf(dto: ReviewCandidateDto): ReviewCandidate {
  return {
    ...dto,
    confidence: orZero(dto.confidence),
    rank: orZero(dto.rank),
    breakdown: {
      similarity: orZero(dto.breakdown.similarity),
      yearPenalty: orZero(dto.breakdown.yearPenalty),
      posterPenalty: orZero(dto.breakdown.posterPenalty),
      evidence: orZero(dto.breakdown.evidence),
    },
  };
}

function itemOf(dto: ReviewItemDto): ReviewItem {
  return { ...dto, candidates: dto.candidates.map(candidateOf) };
}

/** The backend of the desktop app: the review queue lives in the Rust matching service. */
export const tauriReviewBackend: ReviewBackend = {
  available: true,
  list: async () => unwrap(await commands.listReviewItems()).map(itemOf),
  count: async () => unwrap(await commands.countReviewItems()),
  accept: async (reviewId, choice) =>
    void unwrap(await commands.acceptReviewCandidate(reviewId, choice.provider, choice.providerId)),
  acceptTop: async (reviewIds) => unwrap(await commands.acceptTopReviewCandidates([...reviewIds])),
  ignore: async (reviewId) => void unwrap(await commands.ignoreReviewItem(reviewId)),
  reopen: async (reviewIds) => void unwrap(await commands.reopenReviewItems([...reviewIds])),
  search: async (reviewId, query) =>
    unwrap(await commands.searchReviewCandidates(reviewId, query)).map(candidateOf),
  matchItems: async (itemIds) => unwrap(await commands.matchLibraryItems([...itemIds])),
  startBatch: async (mode, itemIds) => {
    const started = unwrap(
      await commands.startMatchBatch(mode, itemIds === null ? null : [...itemIds]),
    );
    return { planned: started.planned, withoutProvider: started.withoutProvider };
  },
};

let override: ReviewBackend | null = null;
let fallback: ReviewBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setReviewBackend(backend: ReviewBackend | null): void {
  override = backend;
}

export function currentReviewBackend(): ReviewBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriReviewBackend;
  // A plain browser has no database: it lists fixtures so the screen can be developed.
  fallback ??= createSyntheticReviewBackend(syntheticReviewItems(), {
    // The same number the sidebar shows: what the pipeline counts say waits for a decision.
    limit: async () => {
      const lifecycle = currentLifecycleBackend();
      const [movies, shows] = await Promise.all([
        lifecycle.counts("movies"),
        lifecycle.counts("shows"),
      ]);
      return movies.uncertain + movies.could_not_match + shows.uncertain + shows.could_not_match;
    },
  });
  return fallback;
}
