import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { useSettingsStore } from "../settings/settingsStore";
import { UpdatesSection } from "../settings/UpdatesSection";
import { AppShell } from "../shell/AppShell";
import { setUpdatesBackend } from "../updates/updatesBackend";
import type { UpdatesBackend } from "../updates/updatesBackend";
import { BUNDLED_CHANGELOG } from "./bundledChangelog";
import { compareVersions, parseChangelog, sectionOf } from "./changelogModel";
import { ReleaseNotesPage } from "./ReleaseNotesPage";
import {
  LAST_SEEN_VERSION_KEY,
  decideWhatsNew,
  markCurrentVersionSeen,
  useWhatsNewStore,
} from "./whatsNew";

const SAMPLE = [
  "# Changelog",
  "",
  "Intro text.",
  "",
  "## [1.2.0] - unreleased",
  "",
  "### Added",
  "",
  "- Faster **scans**.",
  "",
  "## [1.1.0] - 2026-11-01",
  "",
  "### Fixed",
  "- A fix,",
  "  on two lines.",
  "",
  "## Earlier versions",
  "",
  "See the [releases](https://example.test/r).",
].join("\r\n");

describe("reading the changelog", () => {
  it("reads version sections, their dates and plain sections", () => {
    const sections = parseChangelog(SAMPLE);
    expect(sections.map((s) => [s.title, s.version, s.date, s.unreleased])).toEqual([
      ["1.2.0", "1.2.0", null, true],
      ["1.1.0", "1.1.0", "2026-11-01", false],
      ["Earlier versions", null, null, false],
    ]);
    expect(sections[0]?.body).toBe("### Added\n\n- Faster **scans**.");
    expect(sectionOf(sections, "1.1.0")?.body).toContain("A fix,");
    expect(sectionOf(sections, "9.9.9")).toBeNull();
  });

  it("compares versions numerically", () => {
    expect(compareVersions("1.0.9", "1.0.10")).toBeLessThan(0);
    expect(compareVersions("1.2.0", "1.1.9")).toBeGreaterThan(0);
    expect(compareVersions("1.0.0", "1.0.0")).toBe(0);
    expect(compareVersions("dev", "1.0.0")).toBeNull();
  });

  it("bundles the repository's CHANGELOG.md with its released versions", () => {
    const versions = BUNDLED_CHANGELOG.filter((s) => s.version !== null).map((s) => s.version);
    expect(versions).toContain("1.0.1");
    expect(versions).toContain("1.0.0");
    for (const section of BUNDLED_CHANGELOG) expect(section.body).not.toBe("");
  });
});

describe("deciding what to show", () => {
  const sections = parseChangelog(SAMPLE);
  it("shows the new version's section after an update and stores the version", () => {
    const decision = decideWhatsNew({ lastSeen: "1.1.0", current: "1.2.0", sections });
    expect(decision.show?.version).toBe("1.2.0");
    expect(decision.record).toBe(true);
  });
  it("shows nothing on the same version", () => {
    expect(decideWhatsNew({ lastSeen: "1.2.0", current: "1.2.0", sections })).toEqual({
      show: null,
      record: false,
    });
  });
  it("treats an upgrade from a build that stored nothing as an update", () => {
    expect(decideWhatsNew({ lastSeen: undefined, current: "1.1.0", sections }).show?.version).toBe(
      "1.1.0",
    );
  });
  it("stores a downgrade or a version without notes quietly", () => {
    expect(decideWhatsNew({ lastSeen: "1.2.0", current: "1.1.0", sections })).toEqual({
      show: null,
      record: true,
    });
    expect(decideWhatsNew({ lastSeen: "1.2.0", current: "1.3.0", sections })).toEqual({
      show: null,
      record: true,
    });
  });
  it("does nothing while the version is unknown", () => {
    expect(decideWhatsNew({ lastSeen: "1.0.0", current: "", sections })).toEqual({
      show: null,
      record: false,
    });
  });
});

function updatesAt(version: string): UpdatesBackend {
  return {
    available: true,
    status: () =>
      Promise.resolve({
        current_version: version,
        channel_set: false,
        enabled: false,
        offer: null,
      }),
    check: () => Promise.resolve({ outcome: "disabled", offer: null }),
    install: () => Promise.reject(new Error("not used")),
    cancel: () => Promise.resolve(),
    blockingJobs: () => Promise.resolve(0),
  };
}

function renderShell(path = "/library") {
  const rootRoute = createRootRoute({
    component: () => <AppShell platform="windows" windowApi={null} />,
  });
  const routes = [
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/library",
      component: () => <h1>Library page</h1>,
    }),
    createRoute({
      getParentRoute: () => rootRoute,
      path: "/release-notes",
      component: ReleaseNotesPage,
    }),
  ];
  const router = createRouter({
    routeTree: rootRoute.addChildren(routes),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

// The newest released version in the bundled file is the one a test can "update to".
const RELEASED = BUNDLED_CHANGELOG.find((s) => s.version !== null && !s.unreleased)?.version ?? "";

describe("What's new after an update", () => {
  beforeEach(() => {
    useSettingsStore.setState({ values: { [LAST_SEEN_VERSION_KEY]: "0.9.0" }, status: "ready" });
    useWhatsNewStore.setState({ section: null });
    setUpdatesBackend(updatesAt(RELEASED));
  });
  afterEach(() => {
    setUpdatesBackend(null);
    useWhatsNewStore.setState({ section: null });
  });

  it("shows a card with the new version's changes, once", async () => {
    const first = renderShell();
    const card = await screen.findByTestId("whats-new");
    expect(
      within(card).getByRole("heading", { name: `Updated to CineVault ${RELEASED}` }),
    ).toBeVisible();
    expect(within(card).getAllByRole("listitem").length).toBeGreaterThan(2);
    expect(useSettingsStore.getState().values[LAST_SEEN_VERSION_KEY]).toBe(RELEASED);
    first.unmount();

    // The next start, same version: nothing.
    useWhatsNewStore.setState({ section: null });
    renderShell();
    await screen.findByRole("heading", { name: "Library page" });
    await waitFor(() => {
      expect(useSettingsStore.getState().status).toBe("ready");
    });
    expect(screen.queryByTestId("whats-new")).toBeNull();
  });

  it("closes with its button and with Esc, and stays closed", async () => {
    const user = userEvent.setup();
    renderShell();
    const card = await screen.findByTestId("whats-new");
    await user.click(within(card).getAllByRole("button", { name: "Close" })[0] as HTMLElement);
    await waitFor(() => {
      expect(screen.queryByTestId("whats-new")).toBeNull();
    });
    expect(useSettingsStore.getState().values[LAST_SEEN_VERSION_KEY]).toBe(RELEASED);

    const section = sectionOf(BUNDLED_CHANGELOG, RELEASED);
    if (section === null) throw new Error("the bundled changelog has no released version");
    useWhatsNewStore.getState().show(section);
    await screen.findByTestId("whats-new");
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(screen.queryByTestId("whats-new")).toBeNull();
    });
  });

  it("does not show on a fresh install, which stores its version after the setup", async () => {
    useSettingsStore.setState({ values: {}, status: "ready" });
    await markCurrentVersionSeen();
    expect(useSettingsStore.getState().values[LAST_SEEN_VERSION_KEY]).toBe(RELEASED);
    renderShell();
    await screen.findByRole("heading", { name: "Library page" });
    expect(screen.queryByTestId("whats-new")).toBeNull();
  });

  it("opens every version from See all changes", async () => {
    const user = userEvent.setup();
    renderShell();
    await user.click(await screen.findByRole("button", { name: "See all changes" }));
    const page = await screen.findByRole("region", { name: "Release notes" });
    await waitFor(() => {
      expect(screen.queryByTestId("whats-new")).toBeNull();
    });
    const headings = within(page)
      .getAllByRole("heading", { level: 2 })
      .map((h) => h.textContent);
    expect(headings).toEqual(
      BUNDLED_CHANGELOG.map((s) => (s.version === null ? s.title : `Version ${s.version}`)),
    );
    // The installed version is marked, and the only links are https ones.
    expect(within(page).getByText("Installed")).toBeVisible();
    const links = Array.from(page.querySelectorAll("a"));
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) expect(link.getAttribute("href")).toMatch(/^https:\/\//);
  });
});

describe("Settings › Updates", () => {
  it("links to the release notes page", async () => {
    const user = userEvent.setup();
    const rootRoute = createRootRoute({ component: UpdatesSection });
    const notes = createRoute({
      getParentRoute: () => rootRoute,
      path: "/release-notes",
      component: ReleaseNotesPage,
    });
    const home = createRoute({
      getParentRoute: () => rootRoute,
      path: "/",
      component: () => null,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([home, notes]),
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
    await user.click(await screen.findByRole("button", { name: "See all changes" }));
    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/release-notes");
    });
  });
});
