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
import { LibraryPage } from "./LibraryPage";
import { setLibraryBackend } from "./libraryBackend";
import { columnFits, SWITCHABLE_COLUMNS } from "./libraryColumns";
import { resetLibraryStore, useLibraryStore } from "./libraryStore";
import { createSyntheticBackend } from "./syntheticLibrary";
import { paneFitOf } from "./usePaneFit";

/** Lays every element out `width` px wide, as the Library's pane on a phone or a tablet. */
function paneWidth(width: number) {
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => width,
  });
}

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

async function toTable() {
  const user = userEvent.setup();
  renderLibrary();
  await user.click(await screen.findByRole("button", { name: "Table view" }));
  const grid = await screen.findByRole("grid", { name: "Movies table" });
  return { user, grid };
}

const headers = (grid: HTMLElement) =>
  within(grid)
    .getAllByRole("columnheader")
    .map((cell) => cell.getAttribute("aria-label"));

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  resetLibraryStore();
  setLibraryBackend(createSyntheticBackend(40));
});
afterEach(() => {
  restoreLayout();
  setLibraryBackend(null);
});

describe("the fit of the Library's pane", () => {
  it("is wide on the desktop, narrow on a tablet and phone on a phone", () => {
    expect(paneFitOf(0)).toBe("wide");
    expect(paneFitOf(884)).toBe("wide");
    expect(paneFitOf(861)).toBe("wide");
    expect(paneFitOf(860)).toBe("narrow");
    expect(paneFitOf(804)).toBe("narrow");
    expect(paneFitOf(640)).toBe("phone");
    expect(paneFitOf(374)).toBe("phone");
  });

  it("keeps every column on the desktop and leaves the low-value ones out on smaller panes", () => {
    expect(SWITCHABLE_COLUMNS.every((id) => columnFits(id, "wide"))).toBe(true);
    expect(SWITCHABLE_COLUMNS.filter((id) => columnFits(id, "narrow"))).toEqual([
      "stage",
      "year",
      "quality",
      "size",
      "rating",
    ]);
    expect(SWITCHABLE_COLUMNS.filter((id) => columnFits(id, "phone"))).toEqual([
      "stage",
      "quality",
    ]);
  });
});

describe("the Library table on smaller panes", () => {
  it("shows every default column at desktop width", async () => {
    const { grid } = await toTable();
    expect(headers(grid)).toEqual([
      "Select",
      "Stage",
      "Title",
      "Year",
      "Quality",
      "Size",
      "Drive and folder",
      "Rating",
      "Runtime",
    ]);
  });

  it("on a tablet leaves out the folder and runtime, also from the Columns menu", async () => {
    paneWidth(804);
    const { user, grid } = await toTable();
    expect(headers(grid)).toEqual([
      "Select",
      "Stage",
      "Title",
      "Year",
      "Quality",
      "Size",
      "Rating",
    ]);
    await user.click(screen.getByRole("button", { name: "Columns" }));
    expect(await screen.findByRole("menuitemcheckbox", { name: "Rating" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitemcheckbox", { name: "Drive and folder" })).toBeNull();
    expect(screen.queryByRole("menuitemcheckbox", { name: "Runtime" })).toBeNull();
    // The desktop's choice of columns is left as it was.
    expect(useLibraryStore.getState().movies.hiddenColumns).toEqual(["added", "modified"]);
  });

  it("on a phone is a list of title with its year, quality and stage, sorted from the menu", async () => {
    paneWidth(374);
    const { grid } = await toTable();
    expect(headers(grid)).toEqual(["Select", "Title", "Quality", "Stage"]);
    // The year follows the title in its cell.
    const first = within(grid).getAllByRole("row")[1];
    expect(first?.textContent).toMatch(/\d{4}/);
    // No Columns menu: the sort menu sorts by what the list has no column for.
    expect(screen.queryByRole("button", { name: "Columns" })).toBeNull();
    expect(screen.getByRole("button", { name: /Title A–Z/ })).toBeInTheDocument();
  });
});
