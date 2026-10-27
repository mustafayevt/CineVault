import type { QueryClient } from "@tanstack/react-query";

import { t } from "../../i18n/catalog";
import { showToast } from "../toasts/toastStore";
import { currentRatingSweepBackend } from "./ratingSweepBackend";
import type { RatingSweepScope, RatingSweepSummary } from "./ratingSweepBackend";

/** "Fetched ratings for 41 titles. 2 failed. Stopped at the OMDb request limit: …" */
function sweepSentence(summary: RatingSweepSummary): string {
  const parts = [
    summary.updated === 0
      ? t("ratings.sweep.nothingNew")
      : t("ratings.sweep.done", { count: summary.updated }),
  ];
  if (summary.failed > 0) parts.push(t("ratings.sweep.failedCount", { count: summary.failed }));
  if (summary.limited_by !== null) {
    parts.push(t("ratings.sweep.limited", { source: summary.limited_by, count: summary.left }));
  }
  if (summary.cancelled) parts.push(t("ratings.sweep.cancelled"));
  return parts.join(" ");
}

async function refresh(queryClient: QueryClient): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["movie-detail"] }),
    queryClient.invalidateQueries({ queryKey: ["tv-show"] }),
    queryClient.invalidateQueries({ queryKey: ["library"] }),
    queryClient.invalidateQueries({ queryKey: ["rating-sweep-count"] }),
  ]);
}

/**
 * Queues the sweep as one job in Activity and says so; when it is over, the summary toast says
 * what it fetched and, after a request limit, what is left. Resolves with the job id, or `null`
 * after a refusal (said in an error toast).
 */
async function startRatingSweep(
  queryClient: QueryClient,
  scope: RatingSweepScope,
  provider: string | null = null,
): Promise<number | null> {
  try {
    const started = await currentRatingSweepBackend().start(scope, provider, (summary) => {
      showToast({
        message: sweepSentence(summary),
        tone: summary.updated === 0 && summary.failed > 0 ? "error" : "default",
        // A stop at a limit stays until read: it says what is left.
        duration: summary.limited_by !== null || summary.failed > 0 ? null : undefined,
      });
      void refresh(queryClient);
    });
    showToast({ message: t("ratings.sweep.started", { count: started.titles }) });
    return started.job_id;
  } catch (error) {
    showToast({
      message: error instanceof Error ? error.message : String(error),
      tone: "error",
    });
    return null;
  }
}

/**
 * After a new OMDb or MDBList key is saved: when scraped titles lack that source's ratings,
 * offers to fetch them ("Fetch the new OMDb ratings for 24 titles?") with an action that starts
 * the sweep for them. Says nothing when there is nothing to fetch or the count fails.
 */
export async function offerNewKeyRatings(
  queryClient: QueryClient,
  provider: string,
  source: string,
): Promise<void> {
  const backend = currentRatingSweepBackend();
  if (!backend.available) return;
  let count: number;
  try {
    count = await backend.count("missing", provider);
  } catch {
    return;
  }
  if (count === 0) return;
  showToast({
    message: t("ratings.sweep.newKey", { source, count }),
    duration: null,
    action: {
      label: t("ratings.sweep.newKeyAction"),
      onClick: () => void startRatingSweep(queryClient, "missing", provider),
    },
  });
}
