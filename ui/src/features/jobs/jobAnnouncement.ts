import type { JobProgress } from "../../bindings";
import { t } from "../../i18n/catalog";
import { isQuietJob } from "./jobNames";

/** The states a job ends in. */
export const OVER: ReadonlySet<JobProgress["state"]> = new Set([
  "completed",
  "failed",
  "cancelled",
]);

/** What a screen reader hears when `job` has just ended, or `null` when it says nothing. */
export function jobEndAnnouncement(job: JobProgress): string | null {
  if (isQuietJob(job)) return null;
  if (job.state === "failed") return t("job.announce.failed", { name: job.name });
  if (job.state !== "completed") return null;
  return job.failed > 0
    ? t("job.announce.doneWithFailures", { name: job.name, count: job.failed })
    : t("job.announce.done", { name: job.name });
}
