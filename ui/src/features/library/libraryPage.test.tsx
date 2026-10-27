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

import { restoreLayout, stubLayout } from "../../test/layout";
import { validateLibrarySearch } from "../../router";
import { LibraryPage } from "./LibraryPage";
import { setLibraryBackend } from "./libraryBackend";
import { resetLibraryStore, useLibraryStore } from "./libraryStore";
import { createSyntheticBackend } from "./syntheticLibrary";

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
        path: "/movies/$movieId",
        component: () => <p>Movie page</p>,
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
  setLibraryBackend(createSyntheticBackend(181));
});
afterEach(() => {
  restoreLayout();
  setLibraryBackend(null);
});

const footer = async () => {
  const filters = await screen.findByRole("group", { name: "Filters" });
  await waitFor(() => expect(filters.textContent).toMatch(/\d+ movies? ·/));
  return filters.textContent ?? "";
};

describe("the Library page", () => {
  it("opens on Movies with the three types and their counts in one switch", async () => {
    renderLibrary();
    const switcher = await screen.findByRole("group", { name: "Library" });
    expect(within(switcher).getByRole("button", { name: /^Movies/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(within(switcher).getByRole("button", { name: /^TV shows/ })).toBeInTheDocument();
    expect(within(switcher).getByRole("button", { name: /^Collections/ })).toBeInTheDocument();
    await waitFor(() =>
      expect(within(switcher).getByRole("button", { name: /^Movies\s*181/ })).toBeInTheDocument(),
    );
  });

  it("shows the stage chips with counts and the count and size of what is listed", async () => {
    renderLibrary();
    const filters = await screen.findByRole("group", { name: "Filters" });
    await waitFor(() =>
      expect(within(filters).getByRole("button", { name: /^All\s*\d/ })).toBeVisible(),
    );
    for (const name of [
      /^New/,
      /^Scraped/,
      /^Renamed/,
      /^Drive: All/,
      /^Quality: Any/,
      /^Problems/,
    ]) {
      expect(within(filters).getByRole("button", { name })).toBeInTheDocument();
    }
    expect(await footer()).toMatch(/181 movies · /);
  });

  it("narrows the list with a stage chip and keeps the others to hand", async () => {
    const user = userEvent.setup();
    renderLibrary();
    await footer();
    const filters = screen.getByRole("group", { name: "Filters" });
    const newChip = await within(filters).findByRole("button", { name: /^New\s*\d+/ });
    const expected = Number(/(\d+)\s*$/.exec(newChip.textContent ?? "")?.[1]);
    await user.click(newChip);
    expect(newChip).toHaveAttribute("aria-pressed", "true");
    await waitFor(() =>
      expect(filters.textContent).toContain(`${expected} movies`.replace("1 movies", "1 movie")),
    );
  });

  it("lists the union of the problem flags for All problems and shows the search as a chip", async () => {
    const user = userEvent.setup();
    renderLibrary("/library?q=a");
    const filters = await screen.findByRole("group", { name: "Filters" });
    expect(
      await within(filters).findByRole("button", { name: "Clear the search a" }),
    ).toBeInTheDocument();
    await user.click(within(filters).getByRole("button", { name: "Clear the search a" }));
    await waitFor(() =>
      expect(within(filters).queryByRole("button", { name: "Clear the search a" })).toBeNull(),
    );
    await user.click(within(filters).getByTestId("problems-chip"));
    await user.click(await screen.findByRole("menuitemradio", { name: "All problems" }));
    await waitFor(() => expect(filters.textContent).not.toContain("181 movies"));
    expect(useLibraryStore.getState().movies.filters.problems).toBe("all");
    expect(filters.textContent).toMatch(/\d+ movies?/);
  });

  it("remembers the last type, and the view per type", async () => {
    const user = userEvent.setup();
    const router = renderLibrary();
    const switcher = await screen.findByRole("group", { name: "Library" });
    await user.click(within(switcher).getByRole("button", { name: /^TV shows/ }));
    await waitFor(() => expect(router.state.location.search).toMatchObject({ type: "tv" }));
    expect(useLibraryStore.getState().type).toBe("tv");
    await user.click(await screen.findByRole("button", { name: "Table view" }));
    expect(useLibraryStore.getState().tv.view).toBe("table");
    expect(useLibraryStore.getState().movies.view).toBe("grid");
  });
});
