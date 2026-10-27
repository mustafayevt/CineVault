import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { JobProgress } from "../../bindings";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { setLifecycleBackend } from "../lifecycle/lifecycleBackend";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { isMatchJob, newlyFinishedMatchJobs } from "./matchJobs";
import { setReviewBackend } from "./reviewBackend";
import type { ReviewBackend } from "./reviewBackend";
import { createSyntheticReviewBackend } from "./syntheticReview";
import { useRefreshQueueAfterMatch } from "./useRefreshQueue";

function job(overrides: Partial<JobProgress> = {}): JobProgress {
  return {
    id: 1,
    name: "Scraping 6 titles (TMDB)",
    lane: "provider:tmdb",
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

function backendWith(overrides: Partial<ReviewBackend> = {}): ReviewBackend {
  return { ...createSyntheticReviewBackend(), ...overrides };
}

function wrapperFor(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
  useNotificationStore.setState({ items: [] });
  useJobProgressStore.setState({ jobs: [] });
});

afterEach(() => {
  setReviewBackend(null);
  setLifecycleBackend(null);
  vi.restoreAllMocks();
});

describe("the Needs review count after a background match", () => {
  it("recognises the match batch jobs and reports each finish once", () => {
    expect(isMatchJob(job())).toBe(true);
    expect(isMatchJob(job({ name: "Scraping 2 titles (TMDB)" }))).toBe(true);
    expect(isMatchJob(job({ name: "Scanning 3 folders" }))).toBe(false);

    const running = job({ state: "running" });
    const done = job({ state: "completed" });
    expect(newlyFinishedMatchJobs([running], [done])).toEqual([done]);
    expect(newlyFinishedMatchJobs([done], [done])).toEqual([]);
    expect(newlyFinishedMatchJobs([running], [running])).toEqual([]);
    // A job first seen already finished still counts; other jobs never do.
    expect(newlyFinishedMatchJobs([], [done])).toEqual([done]);
    const scan = job({ name: "Scanning 3 folders", state: "completed" });
    expect(newlyFinishedMatchJobs([], [scan])).toEqual([]);
    expect(newlyFinishedMatchJobs([running], [job({ state: "failed" })])).toHaveLength(1);
  });

  it("reads the count and the queue again when a match job reaches a final state", async () => {
    const count = vi.fn(() => Promise.resolve(2));
    const list = vi.fn(() => Promise.resolve([]));
    setReviewBackend(backendWith({ count, list }));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    renderHook(() => useRefreshQueueAfterMatch(), { wrapper: wrapperFor(client) });

    act(() =>
      useJobProgressStore
        .getState()
        .apply({ jobs: [job()], overall: { kind: "idle", percent: 0, active_jobs: 1 } }),
    );
    expect(invalidate).not.toHaveBeenCalled();

    act(() =>
      useJobProgressStore.getState().apply({
        jobs: [job({ state: "completed", done: 10 })],
        overall: { kind: "idle", percent: 100, active_jobs: 0 },
      }),
    );
    await waitFor(() => expect(invalidate).toHaveBeenCalled());
    const keys = invalidate.mock.calls.map((call) => call[0]?.queryKey);
    expect(keys).toContainEqual(["review", "queue"]);
    expect(keys).toContainEqual(["review", "count"]);
  });

  it("ignores jobs that are not match batches", async () => {
    const count = vi.fn(() => Promise.resolve(1));
    setReviewBackend(backendWith({ count }));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    renderHook(() => useRefreshQueueAfterMatch(), { wrapper: wrapperFor(client) });
    act(() =>
      useJobProgressStore.getState().apply({
        jobs: [job({ name: "Scanning 3 folders", state: "completed" })],
        overall: { kind: "idle", percent: 100, active_jobs: 0 },
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(invalidate).not.toHaveBeenCalled();
  });
});
