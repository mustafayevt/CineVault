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

import { validateLibrarySearch } from "../../router";
import { restoreLayout, stubLayout } from "../../test/layout";
import { LibraryPage } from "./LibraryPage";
import { setLibraryBackend } from "./libraryBackend";
import type { LibraryKind } from "./libraryBackend";
import { fixFacets, resetLibraryStore, titleQuery, useLibraryStore } from "./libraryStore";
import { createSyntheticBackend, syntheticRow } from "./syntheticLibrary";

const LIBRARY = 181;

function renderLibrary(path = "/library") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        validateSearch: validateLibrarySearch,
        component: LibraryPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/artwork",
        component: () => <p>Artwork page</p>,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/scrape",
        component: () => <p>Scrape page</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  resetLibraryStore();
  setLibraryBackend(createSyntheticBackend(LIBRARY));
});
afterEach(() => {
  restoreLayout();
  setLibraryBackend(null);
});

/** The fixture rows of a library, to count what each Needs fixing entry should find. */
function rowsOf(kind: Exclude<LibraryKind, "collections">, total: number) {
  return Array.from({ length: total }, (_, i) => syntheticRow(i, kind));
}

const filtersGroup = () => screen.findByRole("group", { name: "Filters" });

/** What the footer of the filter row says it lists, once it has said it. */
async function footerCount(kind: "movie" | "TV show"): Promise<number> {
  const filters = await filtersGroup();
  const pattern = new RegExp(String.raw`^([\d,]+) ${kind}s?\b`);
  const footer = filters.querySelector("[aria-live]") as HTMLElement;
  await waitFor(() => expect(pattern.test(footer.textContent ?? "")).toBe(true));
  return Number(pattern.exec(footer.textContent ?? "")?.[1]?.replaceAll(",", ""));
}

/** Waits until the footer lists exactly `count` titles. */
async function expectListed(kind: "movie" | "TV show", count: number): Promise<void> {
  await waitFor(async () => expect(await footerCount(kind)).toBe(count));
}

async function openMenu(user: ReturnType<typeof userEvent.setup>) {
  const filters = await filtersGroup();
  await user.click(within(filters).getByTestId("needs-fixing-chip"));
  return screen.findByRole("menu", { name: "Find titles that need fixing" });
}

describe("the Needs fixing menu", () => {
  it("sits in the filter row and lists what can be fixed, with a count for each", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const filters = await filtersGroup();
    expect(within(filters).getByRole("button", { name: /^Needs fixing/ })).toBeInTheDocument();
    const menu = await openMenu(user);
    const rows = rowsOf("movies", LIBRARY);
    const noPoster = rows.filter((row) => !row.has_poster).length;
    const noFanart = rows.filter((row) => !row.has_fanart).length;
    const noLogo = rows.filter((row) => !row.has_logo).length;
    const noPlot = rows.filter((row) => row.plot_excerpt === "").length;
    for (const [name, count] of [
      ["No poster", noPoster],
      ["No fan art", noFanart],
      ["No clear logo", noLogo],
      ["No plot", noPlot],
    ] as const) {
      expect(
        await within(menu).findByRole("menuitemcheckbox", { name: `${name} ${count}` }),
      ).toBeInTheDocument();
    }
    for (const name of [/^Not scraped/, /^No title/]) {
      expect(within(menu).getByRole("menuitemcheckbox", { name })).toBeInTheDocument();
    }
  });

  it("lists only the titles without a poster when No poster is ticked, and keeps the menu open", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const menu = await openMenu(user);
    const expected = rowsOf("movies", LIBRARY).filter((row) => !row.has_poster).length;
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No poster/ }));
    expect(useLibraryStore.getState().movies.filters.fix).toEqual(["poster"]);
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await expectListed("movie", expected);
    const filters = await filtersGroup();
    expect(within(filters).getByTestId("needs-fixing-chip")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(within(filters).getByTestId("needs-fixing-chip").textContent).toMatch(/1$/);
  });

  it("combines its entries, the stage, and the typed search", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const menu = await openMenu(user);
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No poster/ }));
    await user.click(within(menu).getByRole("menuitemcheckbox", { name: /^No fan art/ }));
    expect(useLibraryStore.getState().movies.filters.fix).toEqual(["poster", "fanart"]);
    const both = rowsOf("movies", LIBRARY).filter((row) => !row.has_poster && !row.has_fanart);
    await user.keyboard("{Escape}");
    await expectListed("movie", both.length);

    // Not scraped is the New stage: the same titles, narrowed again.
    const newBoth = both.filter((row) => (row.card.lifecycle?.stage ?? "new") === "new");
    await user.click(within(await filtersGroup()).getByTestId("needs-fixing-chip"));
    await user.click(await screen.findByRole("menuitemcheckbox", { name: /^Not scraped/ }));
    expect(useLibraryStore.getState().movies.filters.stage).toBe("new");
    await user.keyboard("{Escape}");
    await waitFor(async () => await expectListed("movie", newBoth.length));
  });

  it("keeps the typed search when an entry is ticked", async () => {
    const user = userEvent.setup();
    renderLibrary("/library?q=the");
    const filters = await filtersGroup();
    expect(
      await within(filters).findByRole("button", { name: "Clear the search the" }),
    ).toBeInTheDocument();
    const searched = await footerCount("movie");
    const menu = await openMenu(user);
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No poster/ }));
    await user.keyboard("{Escape}");
    await waitFor(async () => expect(await footerCount("movie")).toBeLessThanOrEqual(searched));
    expect(
      within(filters).getByRole("button", { name: "Clear the search the" }),
    ).toBeInTheDocument();
  });

  it("says Nothing to fix here when no title is missing what was picked, and clears from there", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const menu = await openMenu(user);
    // No fixture title has an empty title, so this leaves nothing.
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No title/ }));
    await user.keyboard("{Escape}");
    const empty = await screen.findByTestId("nothing-to-fix");
    expect(within(empty).getByText("Nothing to fix here")).toBeInTheDocument();
    await user.click(within(empty).getByRole("button", { name: "Clear Needs fixing" }));
    await waitFor(() => expect(screen.queryByTestId("nothing-to-fix")).toBeNull());
    expect(useLibraryStore.getState().movies.filters.fix).toEqual([]);
    await expectListed("movie", LIBRARY);
  });

  it("clears every choice, the New stage included, from the menu", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const menu = await openMenu(user);
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No plot/ }));
    await user.click(within(menu).getByRole("menuitemcheckbox", { name: /^Not scraped/ }));
    await user.click(within(menu).getByRole("menuitem", { name: "Clear Needs fixing" }));
    const filters = useLibraryStore.getState().movies.filters;
    expect(filters.fix).toEqual([]);
    expect(filters.stage).toBe("all");
  });

  it("works for TV shows by the show's own artwork", async () => {
    const user = userEvent.setup();
    useLibraryStore.getState().setType("tv");
    renderLibrary("/library?type=tv");
    const shows = await createSyntheticBackend(LIBRARY).counts();
    const expected = rowsOf("shows", shows.tv_shows).filter((row) => !row.has_logo).length;
    const menu = await openMenu(user);
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No clear logo/ }));
    expect(useLibraryStore.getState().tv.filters.fix).toEqual(["clearlogo"]);
    expect(useLibraryStore.getState().movies.filters.fix).toEqual([]);
    await user.keyboard("{Escape}");
    await expectListed("TV show", expected);
  });

  it("lets Select all and the bar's Artwork act on the whole result", async () => {
    const user = userEvent.setup();
    const router = renderLibrary();
    const menu = await openMenu(user);
    const missing = rowsOf("movies", LIBRARY).filter((row) => !row.has_poster);
    await user.click(await within(menu).findByRole("menuitemcheckbox", { name: /^No poster/ }));
    await user.keyboard("{Escape}");
    await footerCount("movie");
    const cell = await waitFor(() => {
      const found = document.querySelector<HTMLElement>('[data-index="0"][data-id]');
      if (found === null) throw new Error("no poster yet");
      return found;
    });
    await user.click(cell.querySelector("button") as HTMLElement);
    cell.closest<HTMLElement>("[role='group']")?.focus();
    await user.keyboard("{Control>}a{/Control}");
    const bar = await screen.findByRole("toolbar", { name: "Bulk actions" });
    expect(await within(bar).findByText(`${missing.length} selected`)).toBeInTheDocument();
    await user.click(within(bar).getByRole("button", { name: "Artwork" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    const ids = String((router.state.location.search as { ids?: unknown }).ids).split(",");
    expect(new Set(ids)).toEqual(new Set(missing.map((row) => row.card.id)));
  });
});

describe("the Needs fixing query", () => {
  it("asks the catalogue for all of the picked values of each filter", () => {
    expect(fixFacets([])).toEqual([]);
    const facets = fixFacets(["poster", "clearlogo", "plot"]);
    expect(facets.map((facet) => [facet.id, facet.mode, facet.values])).toEqual([
      ["missing_artwork", "all", ["Poster", "Clear logo"]],
      ["missing_metadata", "all", ["Plot"]],
    ]);
    const query = titleQuery("tv", [], [], "", null, facets);
    expect(query.kind).toBe("shows");
    expect(query.facets).toHaveLength(2);
    // Without choices the query has no facets at all, as before.
    expect(titleQuery("movies", [], [], "", null)).not.toHaveProperty("facets");
  });
});
