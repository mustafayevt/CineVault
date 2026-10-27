import { isTauri } from "@tauri-apps/api/core";
import { useEffect } from "react";

import type { JobProgress } from "../../bindings";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { setJobControls } from "./jobControls";

const DEMO_PARAM = "shell-demo";

/** True in a plain browser whose address asks for the shell demo (screenshots, development). */
export function shellDemo(): boolean {
  return (
    !isTauri() &&
    typeof window !== "undefined" &&
    new URLSearchParams(window.location.search).has(DEMO_PARAM)
  );
}

function demoJob(
  overrides: Partial<JobProgress> & Pick<JobProgress, "id" | "name" | "total" | "done">,
): JobProgress {
  return {
    lane: "disk",
    state: "running",
    priority: "normal",
    failed: 0,
    rolled_back: 0,
    current: [],
    throughput_per_sec: 2,
    eta_secs: 40,
    elapsed_secs: 12,
    outcome: null,
    result: null,
    ...overrides,
  };
}

/**
 * Fills the sidebar with the sample sidebar state in a plain browser when the address
 * carries `?shell-demo`: a scrape and a move running, with Pause and Cancel that act on the demo.
 * The desktop app never runs it.
 */
export function useShellDemo(): void {
  useEffect(() => {
    if (!shellDemo()) return;
    const update = (id: number, change: Partial<JobProgress>) =>
      useJobProgressStore.setState((state) => ({
        jobs: state.jobs.map((job) => (job.id === id ? { ...job, ...change } : job)),
      }));
    setJobControls({
      setPaused: (id, paused) => {
        update(id, { state: paused ? "paused" : "running" });
        return Promise.resolve(true);
      },
      cancel: (id) => {
        update(id, { state: "cancelled" });
        return Promise.resolve(true);
      },
    });
    useJobProgressStore.setState({
      jobs: [
        demoJob({
          id: 1,
          name: "Scraping 25 titles (TMDB)",
          lane: "provider:tmdb",
          total: 25,
          done: 3,
          current: ["Heat (1995): writing artwork"],
        }),
        demoJob({ id: 2, name: "Moving 2 titles to E: Archive", total: 2, done: 1 }),
      ],
      overall: { kind: "normal", percent: 12, active_jobs: 2 },
    });
    return () => setJobControls(null);
  }, []);
}
