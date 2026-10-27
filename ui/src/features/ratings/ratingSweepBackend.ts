import { Channel, isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  RatingSweepScopeDto,
  RatingSweepStartedDto,
  RatingSweepSummaryDto,
} from "../../bindings";

/** Which titles the sweep looks at: those missing a source's ratings, or every scraped title. */
export type RatingSweepScope = RatingSweepScopeDto;
export type RatingSweepStarted = RatingSweepStartedDto;
export type RatingSweepSummary = RatingSweepSummaryDto;

/** A refusal: `code` is `not_found` when there is nothing to fetch, `storage` otherwise. */
class RatingSweepError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "RatingSweepError";
    this.code = code;
  }
}

/** "Fetch missing ratings": the library-wide ratings fetch, one job in Activity. */
export interface RatingSweepBackend {
  available: boolean;
  /**
   * How many titles the sweep of `scope` would fetch. With `provider` (`omdb`, `mdblist`) only
   * those that lack that source's ratings: what a newly saved key unlocks.
   */
  count: (scope: RatingSweepScope, provider: string | null) => Promise<number>;
  /** Queues the sweep; `onDone` receives the summary once, when the job is over. */
  start: (
    scope: RatingSweepScope,
    provider: string | null,
    onDone: (summary: RatingSweepSummary) => void,
  ) => Promise<RatingSweepStarted>;
}

type Result<T> =
  { status: "ok"; data: T } | { status: "error"; error: { code: string; message: string } };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new RatingSweepError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriRatingSweepBackend: RatingSweepBackend = {
  available: true,
  count: async (scope, provider) => unwrap(await commands.countRatingSweep(scope, provider)),
  start: async (scope, provider, onDone) => {
    const channel = new Channel<RatingSweepSummary>();
    channel.onmessage = onDone;
    return unwrap(await commands.startRatingSweep(scope, provider, channel));
  },
};

/**
 * The browser's stand-in (development and screenshots): a library where 37 titles lack ratings
 * and 181 are scraped, and a sweep that ends a moment after it starts.
 */
function createSyntheticRatingSweepBackend(): RatingSweepBackend {
  return {
    available: true,
    count: (scope, provider) =>
      Promise.resolve(scope === "all" ? 181 : provider === null ? 37 : 24),
    start: (scope, provider, onDone) => {
      const titles = scope === "all" ? 181 : provider === null ? 37 : 24;
      window.setTimeout(
        () =>
          onDone({
            job_id: 1,
            updated: titles,
            unchanged: 0,
            skipped: 0,
            failed: 0,
            left: 0,
            limited_by: null,
            cancelled: false,
            sentence: "",
          }),
        1200,
      );
      return Promise.resolve({ job_id: 1, titles });
    },
  };
}

const override: RatingSweepBackend | null = null;
let fallback: RatingSweepBackend | null = null;

export function currentRatingSweepBackend(): RatingSweepBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriRatingSweepBackend;
  fallback ??= createSyntheticRatingSweepBackend();
  return fallback;
}
