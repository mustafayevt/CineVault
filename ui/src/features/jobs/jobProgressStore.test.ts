import { describe, expect, it } from "vitest";

import type { JobProgress } from "../../bindings";
import {
  KEEP_FINISHED_JOBS,
  capFinished,
  mergeJobs,
  useJobProgressStore,
} from "./jobProgressStore";

function job(overrides: Partial<JobProgress> = {}): JobProgress {
  return {
    id: 1,
    name: "Sample job",
    lane: "cpu",
    state: "running",
    priority: "normal",
    total: 10,
    done: 0,
    failed: 0,
    rolled_back: 0,
    current: [],
    throughput_per_sec: null,
    eta_secs: null,
    elapsed_secs: 0,
    outcome: null,
    result: null,
    ...overrides,
  };
}

describe("mergeJobs", () => {
  it("replaces changed jobs, keeps untouched ones and appends new ones in order", () => {
    const a = job({ id: 1, done: 1 });
    const b = job({ id: 2, done: 1 });
    const merged = mergeJobs([a, b], [job({ id: 2, done: 5 }), job({ id: 3 })]);
    expect(merged.map((j) => [j.id, j.done])).toEqual([
      [1, 1],
      [2, 5],
      [3, 0],
    ]);
  });

  it("returns the same array when nothing changed", () => {
    const current = [job()];
    expect(mergeJobs(current, [])).toBe(current);
  });
});

describe("job progress store", () => {
  it("folds a channel message into the jobs and the combined figure", () => {
    useJobProgressStore.setState({
      jobs: [],
      overall: { kind: "idle", percent: 0, active_jobs: 0 },
    });
    useJobProgressStore.getState().apply({
      jobs: [job({ done: 4 })],
      overall: { kind: "normal", percent: 40, active_jobs: 1 },
    });
    expect(useJobProgressStore.getState().jobs[0]?.done).toBe(4);
    expect(useJobProgressStore.getState().overall.percent).toBe(40);
  });

  it("clears finished jobs only", () => {
    useJobProgressStore.setState({
      jobs: [job({ id: 1, state: "completed" }), job({ id: 2, state: "running" })],
    });
    useJobProgressStore.getState().clearFinished();
    expect(useJobProgressStore.getState().jobs.map((j) => j.id)).toEqual([2]);
  });
});

describe("the size of the job list", () => {
  it("keeps the newest finished jobs and every active one", () => {
    const finished = Array.from({ length: KEEP_FINISHED_JOBS + 10 }, (_, n) =>
      job({ id: n + 1, state: "completed" }),
    );
    const running = job({ id: 1000, state: "running" });
    const [oldest, ...rest] = finished;
    const capped = capFinished([...(oldest ? [oldest] : []), running, ...rest]);
    expect(capped).toHaveLength(KEEP_FINISHED_JOBS + 1);
    expect(capped).toContain(running);
    expect(capped.map((j) => j.id)).not.toContain(1);
    expect(capped.at(-1)?.id).toBe(KEEP_FINISHED_JOBS + 10);
  });

  it("keeps the overall object when the figure did not change", () => {
    const overall = { kind: "normal", percent: 40, active_jobs: 1 } as const;
    useJobProgressStore.setState({ jobs: [], overall: { ...overall } });
    const before = useJobProgressStore.getState().overall;
    useJobProgressStore.getState().apply({ jobs: [job({ done: 4 })], overall: { ...overall } });
    expect(useJobProgressStore.getState().overall).toBe(before);
  });
});
