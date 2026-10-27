import type { JobProgress } from "../../bindings";

/**
 * The names the backend gives its jobs come from `cv-app/src/wording.rs`: they use the three
 * verbs (`Scanning 4 folders`, `Scraping 12 titles (TMDB)`) and a count. The UI tells kinds of
 * jobs apart by these words, and a batch started through the API (`Scraping movies from the
 * API`) has no count, so it is neither.
 */
const SCAN = /^Scanning \d/;
const SCRAPE = /^Scraping \d/;

/** Housekeeping the sidebar progress card leaves out. */
const QUIET = [/^Checking names\b/, /^Preparing \d+ thumbnails?\b/];

/** Reading (or reloading) the video details: `Reading media info of 3 files`. */
const MEDIA_INFO = /^(Reading|Reloading) media info\b/;

/** A job that reads video details with MediaInfo. */
export function isMediaInfoJob(job: Pick<JobProgress, "name">): boolean {
  return MEDIA_INFO.test(job.name);
}

/** A scan of library folders. */
export function isScanJob(job: Pick<JobProgress, "name">): boolean {
  return SCAN.test(job.name);
}

/** A scrape batch (one per provider). */
export function isScrapeJob(job: Pick<JobProgress, "name">): boolean {
  return SCRAPE.test(job.name);
}

/** A background job the user has no reason to watch: the name check and thumbnails. */
export function isQuietJob(job: Pick<JobProgress, "name">): boolean {
  return QUIET.some((pattern) => pattern.test(job.name));
}

/** Whether the job is queued, running or paused, that is, not over yet. */
export function isLiveJob(job: Pick<JobProgress, "state">): boolean {
  return job.state === "queued" || job.state === "running" || job.state === "paused";
}
