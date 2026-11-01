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
import { LibraryPage } from "../library/LibraryPage";
import { setLibraryBackend } from "../library/libraryBackend";
import { resetLibraryStore, useLibraryStore } from "../library/libraryStore";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { setCopiesBackend } from "./copiesBackend";
import { createSyntheticCopies, resetSyntheticCopies } from "./syntheticCopies";

function renderRoutes(path: string, children?: React.ReactNode) {
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
        component: () => <>{children ?? <p>Movie page</p>}</>,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId/copies",
        component: () => <p>Compare page</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { router, client };
}

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  resetLibraryStore();
  resetSyntheticCopies();
  setLibraryBackend(createSyntheticBackend(181));
  setCopiesBackend(createSyntheticCopies());
});
afterEach(() => {
  restoreLayout();
  setLibraryBackend(null);
  setCopiesBackend(null);
});

const filters = () => screen.findByRole("group", { name: "Filters" });
const chip = async () => within(await filters()).findByRole("button", { name: /^Copies\s*\d+/ });

describe("the Copies chip", () => {
  it("shows after Problems with the number of movies in open groups", async () => {
    renderRoutes("/library");
    const copies = await chip();

    expect(copies).toHaveAttribute("aria-pressed", "false");
    expect(copies.textContent).toMatch(/Copies\s*4/);
    const names = within(await filters())
      .getAllByRole("button")
      .map((button) => button.textContent ?? "");
    const problems = names.findIndex((name) => name.startsWith("Problems"));
    expect(names[problems + 1]).toMatch(/^Copies/);
  });

  it("lists only the movies of open groups, the copies of a film together", async () => {
    const user = userEvent.setup();
    renderRoutes("/library");
    await user.click(await chip());

    expect(useLibraryStore.getState().movies.filters.copies).toBe(true);
    await waitFor(() =>
      expect(screen.getByRole("group", { name: "Filters" }).textContent).toMatch(/4 movies/),
    );
    await waitFor(() => {
      const names = Array.from(document.querySelectorAll("[data-id]")).map(
        (cell) => cell.querySelector("button")?.textContent ?? "",
      );
      expect(names).toHaveLength(4);
      // Example Movie twice, then Another Film twice: the pairs sit together.
      expect(names[0]).toContain("Another Film");
      expect(names[1]).toContain("Another Film");
      expect(names[2]).toContain("Example Movie");
      expect(names[3]).toContain("Example Movie");
    });
  });

  it("puts the best quality of each film first", async () => {
    const user = userEvent.setup();
    renderRoutes("/library");
    await user.click(await chip());

    await waitFor(() => {
      const tiles = Array.from(document.querySelectorAll("[data-id] button"));
      expect(tiles).toHaveLength(4);
      expect(tiles[0]?.textContent).toContain("2160p");
      expect(tiles[1]?.textContent).toContain("1080p");
    });
  });

  it("is not there for TV shows, nor when no movie has an open copy", async () => {
    const user = userEvent.setup();
    const { client } = renderRoutes("/library");
    await chip();
    await user.click(screen.getByRole("button", { name: /^TV shows/ }));
    await waitFor(() =>
      expect(
        within(screen.getByRole("group", { name: "Filters" })).queryByTestId("copies-chip"),
      ).toBeNull(),
    );

    // The same with every group kept: the movies chip is gone while it is off.
    await createSyntheticCopies().keepAll(["5", "6"]);
    await createSyntheticCopies().keepAll(["31", "32"]);
    await client.invalidateQueries();
    await user.click(screen.getByRole("button", { name: /^Movies/ }));
    await waitFor(() => expect(screen.queryByTestId("copies-chip")).toBeNull());
  });

  it("stays on after the last group is kept and the list says No titles match", async () => {
    const user = userEvent.setup();
    const { client } = renderRoutes("/library");
    await user.click(await chip());
    await waitFor(() => expect(document.querySelectorAll("[data-id]")).toHaveLength(4));

    await createSyntheticCopies().keepAll(["5", "6"]);
    await createSyntheticCopies().keepAll(["31", "32"]);
    // The page reads again after a decision (Keep both invalidates every query).
    await client.invalidateQueries();
    const view = within(await filters());

    await waitFor(() => expect(view.queryByRole("button", { name: /^Copies/ })).not.toBeNull());
    expect(await screen.findByText("No titles match")).toBeInTheDocument();
  });
});

describe("the copies badge", () => {
  it("shows 2 copies on both posters of an open group", async () => {
    renderRoutes("/library?q=example%20movie");
    await waitFor(() => expect(screen.getAllByTestId("copies-badge")).toHaveLength(2));
    for (const badge of screen.getAllByTestId("copies-badge")) {
      expect(badge).toHaveTextContent("2 copies");
    }
  });

  it("is gone for a kept group", async () => {
    await createSyntheticCopies().keepAll(["5", "6"]);
    renderRoutes("/library?q=example%20movie");
    await waitFor(() => expect(document.querySelectorAll("[data-id]").length).toBeGreaterThan(1));
    expect(screen.queryByTestId("copies-badge")).toBeNull();
  });

  it("shows a quiet count after the title in the table", async () => {
    const user = userEvent.setup();
    renderRoutes("/library?q=example%20movie");
    await user.click(await screen.findByRole("button", { name: "Table view" }));
    await waitFor(() => expect(screen.getAllByRole("img", { name: "2 copies" })).toHaveLength(2));
  });
});
