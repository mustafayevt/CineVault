import type { JobProgress } from "../../bindings";
import { isScrapeJob } from "../jobs/jobNames";

const FINAL: ReadonlySet<JobProgress["state"]> = new Set(["completed", "failed", "cancelled"]);

/** Whether the job is one of the match batches. */
export function isMatchJob(job: JobProgress): boolean {
  return isScrapeJob(job);
}

/**
 * The match jobs that reached a final state between two progress snapshots. A job that is seen
 * for the first time already finished counts too (a short batch can end between two updates).
 * Each job is reported once: it is not final in `previous` afterwards.
 */
export function newlyFinishedMatchJobs(
  previous: readonly JobProgress[],
  next: readonly JobProgress[],
): JobProgress[] {
  const before = new Map(previous.map((job) => [job.id, job.state]));
  return next.filter((job) => {
    if (!isMatchJob(job) || !FINAL.has(job.state)) return false;
    const was = before.get(job.id);
    return was === undefined || !FINAL.has(was);
  });
}
