import { create } from "zustand";

import type { JobProgress, JobProgressUpdate, OverallProgress } from "../../bindings";

const IDLE: OverallProgress = { kind: "idle", percent: 0, active_jobs: 0 };

interface JobProgressState {
  /** Latest known progress of every job, in the order the jobs were first seen. */
  jobs: JobProgress[];
  /** Combined progress, the value mirrored to the taskbar and dock. */
  overall: OverallProgress;
  /** Folds one message of the progress channel into the state. */
  apply: (update: JobProgressUpdate) => void;
  /** Drops finished jobs from the list. */
  clearFinished: () => void;
}

const FINISHED: ReadonlySet<JobProgress["state"]> = new Set(["completed", "failed", "cancelled"]);

/** Merges `changed` into `current` by job id, keeping the order of first appearance. */
export function mergeJobs(current: JobProgress[], changed: JobProgress[]): JobProgress[] {
  if (changed.length === 0) {
    return current;
  }
  const incoming = new Map(changed.map((job) => [job.id, job]));
  const merged = current.map((job) => incoming.get(job.id) ?? job);
  const known = new Set(current.map((job) => job.id));
  for (const job of changed) {
    if (!known.has(job.id)) {
      merged.push(job);
    }
  }
  return merged;
}

/** Finished jobs kept in the list, as the backend keeps them; older ones are dropped. */
export const KEEP_FINISHED_JOBS = 50;

/** Drops the oldest finished jobs beyond {@link KEEP_FINISHED_JOBS}; active jobs always stay. */
export function capFinished(jobs: JobProgress[]): JobProgress[] {
  let excess = jobs.filter((job) => FINISHED.has(job.state)).length - KEEP_FINISHED_JOBS;
  if (excess <= 0) return jobs;
  return jobs.filter((job) => {
    if (excess > 0 && FINISHED.has(job.state)) {
      excess -= 1;
      return false;
    }
    return true;
  });
}

/** True when two overall figures show the same thing. */
function sameOverall(a: OverallProgress, b: OverallProgress): boolean {
  return a.kind === b.kind && a.percent === b.percent && a.active_jobs === b.active_jobs;
}

export const useJobProgressStore = create<JobProgressState>((set) => ({
  jobs: [],
  overall: IDLE,
  apply: (update) =>
    set((state) => ({
      jobs: capFinished(mergeJobs(state.jobs, update.jobs)),
      // An unchanged figure keeps its object, so its readers do not re-render.
      overall: sameOverall(state.overall, update.overall) ? state.overall : update.overall,
    })),
  clearFinished: () => set((state) => ({ jobs: state.jobs.filter((j) => !FINISHED.has(j.state)) })),
}));
