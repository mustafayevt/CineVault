import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { validateLibrarySearch } from "../../router";
import { restoreLayout, stubLayout } from "../../test/layout";
import { gridGeometry, POSTER_SIZES } from "./gridGeometry";
import { LibraryPage } from "./LibraryPage";
import { setLibraryBackend } from "./libraryBackend";
import { resetLibraryStore, useLibraryStore } from "./libraryStore";
import { createSyntheticBackend } from "./syntheticLibrary";

function renderLibrary() {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        validateSearch: validateLibrarySearch,
        component: LibraryPage,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
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

/** The poster columns the grid lays out: the grid says so on the element that holds the rows. */
const columnsOf = async () => {
  const scroll = await screen.findByTestId("poster-scroll");
  const holder = scroll.matches("[data-columns]") ? scroll : scroll.querySelector("[data-columns]");
  return Number(holder?.getAttribute("data-columns"));
};

describe("poster size geometry", () => {
  it("fits fewer, wider posters as the size grows and keeps the default as it was", () => {
    const small = gridGeometry(1200, POSTER_SIZES.s);
    const medium = gridGeometry(1200, POSTER_SIZES.m);
    const large = gridGeometry(1200, POSTER_SIZES.l);
    expect(small.columns).toBeGreaterThan(medium.columns);
    expect(medium.columns).toBeGreaterThan(large.columns);
    expect(small.cellWidth).toBeLessThan(medium.cellWidth);
    expect(medium.cellWidth).toBeLessThan(large.cellWidth);
    // Every size fills the row exactly, so nothing jumps at the edges.
    for (const size of [small, medium, large]) {
      expect(size.cellWidth).toBeGreaterThanOrEqual(1);
    }
    expect(gridGeometry(1440)).toEqual(gridGeometry(1440, POSTER_SIZES.m));
  });
});

describe("the poster size control", () => {
  it("sits beside the grid and table switch, in the grid view only", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const sizes = await screen.findByRole("group", { name: "Poster size" });
    expect(
      within(sizes)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["S", "M", "L"]);
    expect(within(sizes).getByRole("button", { name: "Medium posters" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Table view" }));
    expect(screen.queryByRole("group", { name: "Poster size" })).toBeNull();
  });

  it("changes how many posters fit a row and remembers the choice", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const before = await columnsOf();
    await user.click(await screen.findByRole("button", { name: "Large posters" }));
    const large = await columnsOf();
    expect(large).toBeLessThan(before);
    await user.click(screen.getByRole("button", { name: "Small posters" }));
    expect(await columnsOf()).toBeGreaterThan(before);
    expect(useLibraryStore.getState().posterSize).toBe("s");
    const saved = JSON.parse(window.localStorage.getItem("cinevault.library.v1") ?? "{}") as {
      posterSize: string;
    };
    expect(saved.posterSize).toBe("s");
  });

  it("moves with the arrow keys like the other segmented controls", async () => {
    const user = userEvent.setup();
    renderLibrary();
    const medium = await screen.findByRole("button", { name: "Medium posters" });
    medium.focus();
    await user.keyboard("{ArrowRight}");
    expect(useLibraryStore.getState().posterSize).toBe("l");
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(useLibraryStore.getState().posterSize).toBe("s");
  });
});
