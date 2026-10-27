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
import { setCollectionBackend } from "./collectionBackend";
import type { CollectionBackend, CollectionPage } from "./collectionBackend";
import { setLibraryBackend } from "./libraryBackend";
import { resetLibraryStore } from "./libraryStore";
import { createSyntheticBackend, syntheticRow } from "./syntheticLibrary";

const alien: CollectionPage = {
  id: "1",
  title: "Alien Collection",
  sort_title: "alien collection",
  plot: "Science-fiction horror following encounters with the Xenomorph.",
  tmdb_id: "8091",
  poster: null,
  owned: 3,
  total: 5,
  entries: [
    ["10", "Alien", 1979],
    ["11", "Aliens", 1986],
    [null, "Alien³", 1992],
    [null, "Alien Resurrection", 1997],
    ["12", "Alien: Romulus", 2024],
  ].map(([id, title, year]) => ({
    id: id as string | null,
    title: title as string,
    year: year as number,
    release_date: null,
    watched: false,
    poster: null,
    poster_url: null,
    tmdb_id: null,
  })),
};

function renderCollections() {
  const root = createRootRoute();
  const stub = (name: string) => () => <p>{name}</p>;
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
        component: stub("Artwork page"),
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/torrents",
        component: stub("Torrents page"),
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId",
        component: stub("Movie page"),
      }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/library?type=collections"] }),
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
  setLibraryBackend(createSyntheticBackend(181));
  const backend: CollectionBackend = { get: (id) => Promise.resolve({ ...alien, id }) };
  setCollectionBackend(backend);
});
afterEach(() => {
  restoreLayout();
  setLibraryBackend(null);
  setCollectionBackend(null);
});

describe("the Collections view", () => {
  it("lists the collections by name with how many films are owned of each set", async () => {
    renderCollections();
    const grid = await screen.findByRole("group", { name: "Collections", hidden: false });
    const first = syntheticRow(0, "collections");
    await waitFor(() => expect(within(grid).getAllByRole("button").length).toBeGreaterThan(3));
    const names = within(grid)
      .getAllByRole("button")
      .map((tile) => tile.querySelector("span.truncate")?.textContent ?? "");
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
    const owned = first.movies_in_set;
    expect(
      within(grid).getAllByText(`${owned} of ${owned + first.missing_movies} owned`).length,
    ).toBeGreaterThan(0);
  });

  it("opens on the first collection and shows its pane with the owned and the missing films", async () => {
    renderCollections();
    const pane = await screen.findByRole("complementary", { name: "Alien Collection" });
    expect(within(pane).getByRole("button", { name: "Change artwork" })).toBeInTheDocument();
    expect(await within(pane).findByText("Films in this set")).toBeInTheDocument();
    expect(within(pane).getByText(/\d+ of \d+ owned/)).toBeInTheDocument();
    expect(within(pane).getByText(/Science-fiction horror/)).toBeInTheDocument();

    // Owned films open their page.
    const alienLink = within(pane).getByRole("link", { name: /^Alien\s*\(1979\)/ });
    expect(alienLink).toHaveAttribute("href", "/movies/10");
    expect(alienLink.closest("li")).toHaveAttribute("data-owned", "true");

    // Missing films are muted and offer a torrent search for them.
    const missing = within(pane)
      .getAllByRole("listitem")
      .filter((item) => item.getAttribute("data-owned") === "false");
    expect(missing).toHaveLength(2);
    const search = within(missing[0] as HTMLElement).getByRole("link", { name: "Search torrents" });
    expect(search).toHaveAttribute("href", expect.stringContaining("/torrents?q="));
    expect(new URL(search.getAttribute("href") ?? "", "http://app").searchParams.get("q")).toBe(
      "Alien³ 1992",
    );
  });

  it("sends Change artwork to the artwork picker for that collection", async () => {
    const user = userEvent.setup();
    const router = renderCollections();
    const pane = await screen.findByRole("complementary");
    await user.click(within(pane).getByRole("button", { name: "Change artwork" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(router.state.location.search).toMatchObject({ collection: expect.any(String) });
  });

  it("shows another collection's pane when its tile is chosen", async () => {
    const user = userEvent.setup();
    renderCollections();
    const grid = await screen.findByRole("group", { name: "Collections" });
    await waitFor(() => expect(within(grid).getAllByRole("button").length).toBeGreaterThan(3));
    const second = within(grid).getAllByRole("button")[1] as HTMLElement;
    await user.click(second);
    expect(second).toHaveAttribute("aria-pressed", "true");
    expect(within(grid).getAllByRole("button")[0]).toHaveAttribute("aria-pressed", "false");
  });
});
