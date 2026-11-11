import type { JobProgress, JobUnitsDto } from "../../bindings";
import { t } from "../../i18n/catalog";
import { isLiveJob, isQuietJob } from "../jobs/jobNames";

/** The jobs the card is about, oldest first: the work the user started, not housekeeping. */
export function visibleJobs(jobs: readonly JobProgress[]): JobProgress[] {
  return jobs.filter((job) => isLiveJob(job) && !isQuietJob(job));
}

/** The units a job counts in, when it counts more than one (a scan of one title has none). */
function unitsOf(job: Pick<JobProgress, "units">): JobUnitsDto | null {
  return job.units ?? null;
}

/**
 * 0 to 100 for one job; a job of unknown size is at 0. A scan counts its titles, not its
 * library folders, and is at 0 while it is still finding them.
 */
export function jobPercent(job: Pick<JobProgress, "done" | "total" | "units">): number {
  const units = unitsOf(job);
  if (units !== null) {
    if (units.phase === "counting" || units.total === 0) return 0;
    return Math.min(100, (units.done / units.total) * 100);
  }
  return job.total > 0 ? Math.min(100, (job.done / job.total) * 100) : 0;
}

/** "about 3 min left", "about 1 h 5 min left", "less than a minute left". */
export function scanTimeLeftText(seconds: number): string {
  if (seconds < 60) return t("scan.progress.left.underMinute");
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return t("scan.progress.left.minutes", { count: minutes });
  return t("scan.progress.left.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}

/**
 * The card's bold line: the job's verb with its count ("Scraping 3 of 25") when the backend
 * names it "<verb>ing …" and knows its size; otherwise the job's own name. A scan counts titles:
 * "Finding titles… 312 found", then "Scanning 312 of 707 titles"; a scan of one title keeps its
 * name ("Scanning Example Movie (2020)").
 */
export function jobLine(
  job: Pick<JobProgress, "name" | "done" | "total" | "state" | "units">,
): string {
  const verb = /^(\p{L}+ing)\b/u.exec(job.name)?.[1];
  const units = unitsOf(job);
  let line: string;
  if (units !== null && units.phase === "counting") {
    line = t("scan.progress.finding", { count: units.total });
  } else if (units !== null) {
    line =
      verb !== undefined && units.total > 1
        ? t("scan.progress.titles", { verb, done: units.done, count: units.total })
        : job.name;
  } else {
    line =
      verb !== undefined && job.total > 0
        ? t("shell.progress.line", { verb, done: job.done, total: job.total })
        : job.name;
  }
  return job.state === "paused" ? t("shell.progress.paused", { line }) : line;
}

/**
 * The muted line under a job in the running-work list: "3 of 25 · 12%"; for a scan "312 found"
 * while it counts, then "44% · about 3 min left" (the time once it is known). `null` while the
 * size is unknown.
 */
export function jobCountText(job: Pick<JobProgress, "done" | "total" | "units">): string | null {
  const units = unitsOf(job);
  const percent = Math.round(jobPercent(job));
  if (units !== null) {
    if (units.phase === "counting") return t("scan.progress.found", { count: units.total });
    if (units.total <= 1) return null;
    const eta = units.eta_secs;
    return eta === null || eta === undefined
      ? `${percent}%`
      : `${percent}% · ${scanTimeLeftText(eta)}`;
  }
  return job.total > 0
    ? t("shell.progress.count", { done: job.done, total: job.total, percent })
    : null;
}

/** What a job is reading now: the title or folder of a scan, else the job's first item. */
export function jobCurrent(job: Pick<JobProgress, "current" | "units">): string | undefined {
  const current = unitsOf(job)?.current;
  return current === null || current === undefined || current === "" ? job.current[0] : current;
}

/** The card's bold line for a move: its name, with "(paused)" while it is held. */
export function moveLine(name: string, paused: boolean): string {
  return paused ? t("shell.progress.paused", { line: name }) : name;
}
