import { render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { JobProgress, JobUnitsDto } from "../../bindings";
import { RunningNow } from "../activity/RunningNow";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { useRenameRunStore } from "../rename/runRename";
import { jobCountText, jobCurrent, jobLine, jobPercent, scanTimeLeftText } from "./progressLines";

function job(overrides: Partial<JobProgress> & Pick<JobProgress, "id" | "name">): JobProgress {
  return {
    lane: "disk",
    state: "running",
    priority: "normal",
    total: 8,
    done: 0,
    failed: 0,
    rolled_back: 0,
    current: ["Movies 4K"],
    throughput_per_sec: null,
    eta_secs: null,
    elapsed_secs: 4,
    outcome: null,
    result: null,
    ...overrides,
  };
}

function units(overrides: Partial<JobUnitsDto>): JobUnitsDto {
  return {
    phase: "working",
    done: 312,
    total: 707,
    current: "Example Movie (2020)",
    eta_secs: null,
    ...overrides,
  };
}

afterEach(() => {
  useJobProgressStore.setState({ jobs: [] });
  useRenameRunStore.setState({ running: null });
});

describe("a scan's progress in titles", () => {
  it("reads Finding titles while it counts, at 0%, naming the folder being listed", () => {
    const counting = job({
      id: 1,
      name: "Scanning 8 folders",
      units: units({ phase: "counting", done: 0, total: 312, current: "Movies 4K" }),
    });
    expect(jobLine(counting)).toBe("Finding titles… 312 found");
    expect(jobPercent(counting)).toBe(0);
    expect(jobCountText(counting)).toBe("312 found");
    expect(jobCurrent(counting)).toBe("Movies 4K");
  });

  it("counts titles, not folders, once it reads them", () => {
    const working = job({ id: 1, name: "Scanning 8 folders", units: units({}) });
    expect(jobLine(working)).toBe("Scanning 312 of 707 titles");
    expect(Math.round(jobPercent(working))).toBe(44);
    expect(jobCountText(working)).toBe("44%");
    expect(jobCurrent(working)).toBe("Example Movie (2020)");
  });

  it("adds the time left only once the backend knows it", () => {
    const timed = job({ id: 1, name: "Scanning 8 folders", units: units({ eta_secs: 175 }) });
    expect(jobCountText(timed)).toBe("44% · about 3 min left");
    expect(scanTimeLeftText(20)).toBe("less than a minute left");
    expect(scanTimeLeftText(3_900)).toBe("about 1 h 5 min left");
  });

  it("keeps the name of a scan of one title", () => {
    const one = job({
      id: 1,
      name: "Scanning Example Movie (2020)",
      total: 1,
      units: units({ done: 0, total: 1 }),
    });
    expect(jobLine(one)).toBe("Scanning Example Movie (2020)");
    expect(jobCountText(one)).toBeNull();
  });

  it("leaves other jobs as they were", () => {
    const scrape = job({ id: 2, name: "Scraping 25 titles (TMDB)", total: 25, done: 3 });
    expect(jobLine(scrape)).toBe("Scraping 3 of 25");
    expect(jobCountText(scrape)).toBe("3 of 25 · 12%");
  });
});

describe("Running now in Activity", () => {
  it("is gone while nothing runs", () => {
    render(<RunningNow />);
    expect(screen.queryByTestId("running-now")).toBeNull();
  });

  it("shows a scan and a rename with the same lines as the sidebar", () => {
    useJobProgressStore.setState({
      jobs: [job({ id: 1, name: "Scanning 8 folders", units: units({ eta_secs: 175 }) })],
    });
    useRenameRunStore.setState({
      running: { name: "Renaming 12 titles", done: 3, total: 12, title: "Example Movie" },
    });
    render(<RunningNow />);
    const section = screen.getByRole("region", { name: "Running now" });
    expect(within(section).getByText("Scanning 8 folders")).toBeVisible();
    expect(
      within(section).getByText("44% · about 3 min left · Example Movie (2020)"),
    ).toBeVisible();
    expect(within(section).getByRole("button", { name: "Pause Scanning 8 folders" })).toBeVisible();
    // A rename shows its progress without Pause or Cancel.
    expect(within(section).getByText("Renaming 12 titles")).toBeVisible();
    expect(within(section).getByText("3 of 12 · 25% · Example Movie")).toBeVisible();
    expect(within(section).queryByRole("button", { name: /Renaming 12 titles/ })).toBeNull();
  });
});
