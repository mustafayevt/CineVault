import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { JobProgress, LifecycleCountsDto } from "../../bindings";
import { useShellStore } from "../../stores/shell";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { setLifecycleBackend } from "../lifecycle/lifecycleBackend";
import type { PipelineKind } from "../lifecycle/lifecycleBackend";
import { SIDEBAR_SETTING_KEY, useSettingsStore } from "../settings/settingsStore";
import { DriveMeters } from "./DriveMeters";
import { destinationOf } from "./destinations";
import { setJobControls } from "./jobControls";
import { jobLine, visibleJobs } from "./progressLines";
import { SidebarPanel } from "./SidebarPanel";
import type { SidebarDrive } from "./useSidebarDrives";

function counts(overrides: Partial<LifecycleCountsDto>): LifecycleCountsDto {
  return {
    total: 0,
    new: 0,
    scraped: 0,
    renamed: 0,
    needs_review: 0,
    uncertain: 0,
    not_matched: 0,
    not_scraped: 0,
    could_not_match: 0,
    not_matched_top_reason: null,
    working: 0,
    rename_pending: 0,
    locked: 0,
    missing: 0,
    checking: 0,
    ...overrides,
  };
}

function job(overrides: Partial<JobProgress> & Pick<JobProgress, "id" | "name">): JobProgress {
  return {
    lane: "disk",
    state: "running",
    priority: "normal",
    total: 25,
    done: 3,
    failed: 0,
    rolled_back: 0,
    current: [],
    throughput_per_sec: 1,
    eta_secs: 10,
    elapsed_secs: 4,
    outcome: null,
    result: null,
    ...overrides,
  };
}

const PAGES = ["/library", "/scrape", "/rename", "/drives", "/torrents", "/history", "/settings"];

function renderSidebar(path = "/library", platform: "windows" | "macos" = "windows") {
  const rootRoute = createRootRoute({ component: () => <SidebarPanel platform={platform} /> });
  const routes = [...PAGES, "/movies/$movieId"].map((to) =>
    createRoute({ getParentRoute: () => rootRoute, path: to, component: () => null }),
  );
  const router = createRouter({
    routeTree: rootRoute.addChildren(routes),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

beforeEach(() => {
  setLifecycleBackend({
    counts: (kind: PipelineKind) =>
      Promise.resolve(
        kind === "movies"
          ? counts({ not_scraped: 7, uncertain: 2, not_matched: 1, rename_pending: 60 })
          : counts({ not_scraped: 2, rename_pending: 11 }),
      ),
    get: () => Promise.resolve([]),
  });
  useShellStore.setState({ sidebarOverride: null, narrowWindow: false, theme: "dark" });
  useJobProgressStore.setState({ jobs: [] });
});

afterEach(() => {
  setLifecycleBackend(null);
  setJobControls(null);
});

describe("the sidebar destinations", () => {
  it("lists Library, Scrape, Rename, Drives and Torrents in the panel", async () => {
    renderSidebar();
    const panel = await screen.findByRole("navigation", { name: "Main" });
    const names = within(panel)
      .getAllByRole("link")
      .map((link) => link.textContent?.replace(/\d+( low)?$/, ""));
    expect(names.slice(0, 5)).toEqual(["Library", "Scrape", "Rename", "Drives", "Torrents"]);
  });

  it("shows the waiting counts of movies and shows together as soft pills", async () => {
    renderSidebar();
    // Scrape = not scraped + uncertain + not matched (7 + 2 + 1, and 2 shows).
    expect(await screen.findByRole("link", { name: "Scrape, 12" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Rename, 71" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Library" })).toBeInTheDocument();
  });

  it("marks only the page being shown as active, with a gold icon", async () => {
    renderSidebar("/rename");
    const rename = await screen.findByRole("link", { name: /^Rename/ });
    expect(rename).toHaveAttribute("aria-current", "page");
    expect(rename.querySelector("svg")?.getAttribute("class")).toContain("text-gold");
    const current = screen.getAllByRole("link").filter((link) => link.hasAttribute("aria-current"));
    expect(current).toEqual([rename]);
  });

  it("counts a title page as part of the Library", () => {
    expect(destinationOf("/movies/12")).toBe("library");
    expect(destinationOf("/tv/3")).toBe("library");
    expect(destinationOf("/scrape/match/4")).toBe("scrape");
    expect(destinationOf("/settings")).toBe("settings");
    expect(destinationOf("/welcome")).toBeNull();
  });

  it("keeps History and Settings in the icon row, active on their pages", async () => {
    renderSidebar("/history");
    const history = await screen.findByRole("link", { name: "History" });
    expect(history).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Settings" })).not.toHaveAttribute("aria-current");
    // The active icon is gold on the active fill; the ghost grey must not fight it.
    expect(history.className).toContain("text-gold");
    expect(history.className).toContain("bg-surface-active");
    expect(history.className).not.toContain("text-text-2");
    expect(screen.getByRole("link", { name: "Settings" }).className).toContain("text-text-2");
  });

  it("switches the theme from the icon row", async () => {
    renderSidebar();
    await userEvent
      .setup()
      .click(await screen.findByRole("button", { name: "Switch to the light theme" }));
    expect(useShellStore.getState().theme).toBe("light");
    expect(screen.getByRole("button", { name: "Switch to the dark theme" })).toBeInTheDocument();
  });
});

describe("collapsing the sidebar", () => {
  it("collapses to icons with named tooltips and remembers the choice", async () => {
    const user = userEvent.setup();
    renderSidebar();
    await user.click(await screen.findByRole("button", { name: "Collapse sidebar" }));
    expect(screen.getByRole("navigation", { name: "Main" })).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(useSettingsStore.getState().values[SIDEBAR_SETTING_KEY]).toBe("collapsed");
    // Icon only: the label and the count are the accessible name and the tooltip.
    const scrape = await screen.findByRole("link", { name: "Scrape, 12" });
    expect(scrape).not.toHaveTextContent("Scrape");
    fireEvent.focus(scrape);
    expect((await screen.findAllByText("Scrape · 12")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Expand sidebar" }));
    expect(useSettingsStore.getState().values[SIDEBAR_SETTING_KEY]).toBe("expanded");
  });

  it("opens collapsed when the saved choice says so", async () => {
    useShellStore.setState({ sidebarOverride: "collapsed" });
    renderSidebar();
    expect(await screen.findByRole("navigation", { name: "Main" })).toHaveAttribute(
      "data-collapsed",
      "true",
    );
    expect(screen.queryByText("CineVault")).toBeNull();
  });
});

describe("back and forward", () => {
  it("goes back and forward from the logo row, Alt+arrows and the mouse side buttons", async () => {
    const user = userEvent.setup();
    const router = renderSidebar("/library");
    await act(() => router.navigate({ to: "/scrape" }));
    await act(() => router.navigate({ to: "/rename" }));
    await user.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/scrape"));
    fireEvent.keyDown(window, { key: "ArrowLeft", altKey: true });
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    fireEvent.mouseUp(window, { button: 4 });
    await waitFor(() => expect(router.state.location.pathname).toBe("/scrape"));
    fireEvent.keyDown(window, { key: "ArrowRight", altKey: true });
    await waitFor(() => expect(router.state.location.pathname).toBe("/rename"));
    fireEvent.mouseUp(window, { button: 3 });
    await waitFor(() => expect(router.state.location.pathname).toBe("/scrape"));
  });

  it("stops the webview's own handling, so a press never moves twice", async () => {
    renderSidebar("/library");
    await screen.findByRole("navigation", { name: "Main" });
    const key = new KeyboardEvent("keydown", { key: "ArrowLeft", altKey: true, cancelable: true });
    window.dispatchEvent(key);
    expect(key.defaultPrevented).toBe(true);
    const side = new MouseEvent("mousedown", { button: 3, cancelable: true });
    window.dispatchEvent(side);
    expect(side.defaultPrevented).toBe(true);
  });

  it("leaves Alt+arrows to a text field", async () => {
    const router = renderSidebar("/library");
    await act(() => router.navigate({ to: "/scrape" }));
    const field = document.createElement("input");
    document.body.append(field);
    fireEvent.keyDown(field, { key: "ArrowLeft", altKey: true });
    expect(router.state.location.pathname).toBe("/scrape");
    field.remove();
  });
});

describe("the progress card", () => {
  it("is gone while nothing runs", async () => {
    renderSidebar();
    await screen.findByRole("navigation", { name: "Main" });
    expect(screen.queryByTestId("sidebar-progress")).toBeNull();
    expect(screen.getByRole("link", { name: "History" })).toBeInTheDocument();
  });

  it("names the job started first with its count, percentage and +1 more", async () => {
    useJobProgressStore.setState({
      jobs: [
        job({ id: 1, name: "Scraping 25 titles (TMDB)", done: 3, total: 25 }),
        job({ id: 2, name: "Moving 2 titles to E: Archive", done: 1, total: 2 }),
        job({ id: 3, name: "Checking names of 40 titles", done: 1, total: 40 }),
      ],
    });
    renderSidebar();
    const card = await screen.findByTestId("sidebar-progress");
    expect(card).toHaveTextContent("Scraping 3 of 25");
    expect(card).toHaveTextContent("12%");
    expect(card).toHaveTextContent("+1 more");
  });

  it("lists the running jobs with Pause and Cancel", async () => {
    const controls = {
      setPaused: vi.fn().mockResolvedValue(true),
      cancel: vi.fn().mockResolvedValue(true),
    };
    setJobControls(controls);
    useJobProgressStore.setState({
      jobs: [
        job({ id: 1, name: "Scraping 25 titles (TMDB)" }),
        job({ id: 2, name: "Moving 2 titles to E: Archive", state: "paused", done: 1, total: 2 }),
      ],
    });
    const user = userEvent.setup();
    renderSidebar();
    await user.click(await screen.findByTestId("sidebar-progress"));
    const list = await screen.findByRole("dialog", { name: "Running work" });
    await user.click(within(list).getByRole("button", { name: "Pause Scraping 25 titles (TMDB)" }));
    expect(controls.setPaused).toHaveBeenCalledWith(1, true);
    await user.click(
      within(list).getByRole("button", { name: "Resume Moving 2 titles to E: Archive" }),
    );
    expect(controls.setPaused).toHaveBeenCalledWith(2, false);
    await user.click(
      within(list).getByRole("button", { name: "Cancel Moving 2 titles to E: Archive" }),
    );
    expect(controls.cancel).toHaveBeenCalledWith(2);
  });

  it("becomes a ring when the sidebar is collapsed", async () => {
    useShellStore.setState({ sidebarOverride: "collapsed" });
    useJobProgressStore.setState({ jobs: [job({ id: 1, name: "Scraping 25 titles (TMDB)" })] });
    renderSidebar();
    const card = await screen.findByTestId("sidebar-progress");
    expect(within(card).getByTestId("progress-ring")).toBeInTheDocument();
  });

  it("words a job by its verb and count, or by its name when the size is unknown", () => {
    expect(jobLine({ name: "Scanning 4 folders", done: 1, total: 4, state: "running" })).toBe(
      "Scanning 1 of 4",
    );
    expect(
      jobLine({ name: "Scraping movies from the API", done: 0, total: 0, state: "running" }),
    ).toBe("Scraping movies from the API");
    expect(jobLine({ name: "Scanning 4 folders", done: 1, total: 4, state: "paused" })).toBe(
      "Scanning 1 of 4 (paused)",
    );
    expect(
      visibleJobs([
        job({ id: 1, name: "Scanning 4 folders", state: "completed" }),
        job({ id: 2, name: "Preparing 30 thumbnails" }),
      ]),
    ).toEqual([]);
  });
});

describe("the drives section", () => {
  const drives: SidebarDrive[] = [
    { id: "c", name: "C: System", totalBytes: 931e9, freeBytes: 41e9, online: true, low: true },
    { id: "z", name: "Z: NAS", totalBytes: 16e12, freeBytes: 0, online: false, low: false },
  ];

  function renderDrives(list: readonly SidebarDrive[], collapsed = false) {
    const rootRoute = createRootRoute({
      component: () => <DriveMeters drives={list} collapsed={collapsed} />,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([
        createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null }),
        createRoute({ getParentRoute: () => rootRoute, path: "/drives", component: () => null }),
      ]),
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });
    render(<RouterProvider router={router} />);
  }

  it("shows nothing without drives (the stub until the drives service lands)", async () => {
    renderDrives([]);
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Drives" })).toBeNull());
  });

  it("shows free space in the warning colour for a low drive and greys an offline one", async () => {
    renderDrives(drives);
    const low = await screen.findByText(/^41\sGB free$/);
    expect(low.className).toContain("text-warn");
    expect(screen.getByText("Offline").className).toContain("text-text-disabled");
    const meters = screen.getAllByRole("meter");
    expect(meters[0]).toHaveAttribute("data-state", "low");
    expect(meters[1]).toHaveAttribute("data-state", "offline");
    expect(meters[0]).toHaveAccessibleName(/^C: System · 41\sGB free of 931\sGB$/);
    expect(screen.getAllByRole("link")[0]).toHaveAttribute("href", "/drives");
  });

  it("shrinks to small meters named by their tooltip when collapsed", async () => {
    renderDrives(drives, true);
    expect(
      await screen.findByRole("link", { name: /^C: System · 41\sGB free of 931\sGB$/ }),
    ).toBeInTheDocument();
    expect(screen.queryByText(/^41\sGB free$/)).toBeNull();
  });
});
