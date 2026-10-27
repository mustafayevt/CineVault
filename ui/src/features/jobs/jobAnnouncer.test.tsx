import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { JobProgress } from "../../bindings";
import { JobAnnouncer } from "./JobAnnouncer";
import { useJobProgressStore } from "./jobProgressStore";

const IDLE = { kind: "idle" as const, percent: 0, active_jobs: 0 };

function job(over: Partial<JobProgress> = {}): JobProgress {
  return {
    id: 1,
    name: "Scraping 12 titles",
    lane: "provider:tmdb",
    state: "running",
    priority: "normal",
    total: 12,
    done: 3,
    failed: 0,
    rolled_back: 0,
    current: [],
    throughput_per_sec: null,
    eta_secs: null,
    elapsed_secs: null,
    outcome: null,
    ...over,
  } as JobProgress;
}

function push(...jobs: JobProgress[]) {
  act(() => useJobProgressStore.getState().apply({ jobs, overall: IDLE }));
}

const announced = () => screen.getByRole("status").textContent ?? "";

afterEach(() => {
  useJobProgressStore.setState({ jobs: [], overall: IDLE });
});

describe("the job announcer", () => {
  it("says when a job finishes and when one fails, politely, and not on every tick", () => {
    render(<JobAnnouncer />);
    const region = screen.getByRole("status");
    expect(region).toHaveAttribute("aria-live", "polite");
    push(job({ id: 1 }), job({ id: 2, name: "Scanning 2 folders", lane: "disk" }));
    push(job({ id: 1, done: 6 }));
    push(job({ id: 1, done: 9 }));
    expect(announced()).toBe("");

    push(job({ id: 1, done: 12, state: "completed" }));
    expect(announced()).toBe("Scraping 12 titles: finished");

    push(job({ id: 2, name: "Scanning 2 folders", lane: "disk", state: "failed" }));
    expect(announced()).toBe("Scanning 2 folders: failed.");
  });

  it("names the items that failed in a finished job", () => {
    render(<JobAnnouncer />);
    push(job());
    push(job({ state: "completed", done: 12, failed: 2 }));
    expect(announced()).toBe("Scraping 12 titles: finished, 2 items failed");
  });

  it("stays quiet about jobs that were over when the app opened, cancels and housekeeping", () => {
    useJobProgressStore.setState({
      jobs: [job({ id: 7, state: "completed" })],
      overall: IDLE,
    });
    render(<JobAnnouncer />);
    push(job({ id: 3 }), job({ id: 4, name: "Checking names of 12 titles" }));
    push(
      job({ id: 3, state: "cancelled" }),
      job({ id: 4, name: "Checking names of 12 titles", state: "completed" }),
    );
    expect(announced()).toBe("");
  });

  it("reads the first list it gets as the state at start", () => {
    render(<JobAnnouncer />);
    // The first message of the channel lists every job, including ones that ended earlier.
    push(job({ id: 9, state: "completed" }));
    expect(announced()).toBe("");
    // A job that starts and ends after that is announced, even when it was never seen running.
    push(job({ id: 10, name: "Renaming 3 folders", state: "completed" }));
    expect(announced()).toBe("Renaming 3 folders: finished");
  });
});
