import type { JobProgress } from "../../bindings";
import { t } from "../../i18n/catalog";
import { isLiveJob, isQuietJob } from "../jobs/jobNames";

/** The jobs the card is about, oldest first: the work the user started, not housekeeping. */
export function visibleJobs(jobs: readonly JobProgress[]): JobProgress[] {
  return jobs.filter((job) => isLiveJob(job) && !isQuietJob(job));
}

/** 0 to 100 for one job; a job of unknown size is at 0. */
export function jobPercent(job: JobProgress): number {
  return job.total > 0 ? Math.min(100, (job.done / job.total) * 100) : 0;
}

/**
 * The card's bold line: the job's verb with its count ("Scraping 3 of 25") when the backend
 * names it "<verb>ing …" and knows its size; otherwise the job's own name.
 */
export function jobLine(job: Pick<JobProgress, "name" | "done" | "total" | "state">): string {
  const verb = /^(\p{L}+ing)\b/u.exec(job.name)?.[1];
  const line =
    verb !== undefined && job.total > 0
      ? t("shell.progress.line", { verb, done: job.done, total: job.total })
      : job.name;
  return job.state === "paused" ? t("shell.progress.paused", { line }) : line;
}

/** The card's bold line for a move: its name, with "(paused)" while it is held. */
export function moveLine(name: string, paused: boolean): string {
  return paused ? t("shell.progress.paused", { line: name }) : name;
}
