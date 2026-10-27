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
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { validateLibrarySearch } from "../../router";
import { restoreLayout, stubLayout } from "../../test/layout";
import { LibraryPage } from "./LibraryPage";
import { setLibraryBackend } from "./libraryBackend";
import type { LibraryQuery } from "./libraryBackend";
import { COLUMN_SORT, columnOfField, SWITCHABLE_COLUMNS } from "./libraryColumns";
import { resetLibraryStore, useLibraryStore } from "./libraryStore";
import { SORT_CHOICES, SORT_KEYS, sortChoiceOf } from "./sortChoices";
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

async function toTable() {
  const user = userEvent.setup();
  renderLibrary();
  await user.click(await screen.findByRole("button", { name: "Table view" }));
  const grid = await screen.findByRole("grid", { name: "Movies table" });
  return { user, grid };
}

const header = (grid: HTMLElement, name: string) =>
  within(grid).getByRole("columnheader", { name });

/** The headers that show a sort: "Name:direction". */
const sorted = (grid: HTMLElement) =>
  within(grid)
    .getAllByRole("columnheader")
    .filter((h) => ["ascending", "descending"].includes(h.getAttribute("aria-sort") ?? ""))
    .map((h) => `${h.getAttribute("aria-label")}:${h.getAttribute("aria-sort")}`);

const savedLibrary = () =>
  JSON.parse(window.localStorage.getItem("cinevault.library.v1") ?? "{}") as {
    movies: { sort: unknown; hiddenColumns: string[] };
  };

describe("sorting the Library table by its headers", () => {
  it("gives every column a backend sort of its own", () => {
    const fields = Object.values(COLUMN_SORT);
    expect(new Set(fields).size).toBe(fields.length);
    for (const [column, field] of Object.entries(COLUMN_SORT)) {
      expect(columnOfField(field)).toBe(column);
    }
  });

  it("sorts on the first click, flips on the second and marks only that header", async () => {
    const { user, grid } = await toTable();
    expect(sorted(grid)).toEqual(["Title:ascending"]);
    const year = within(grid).getByRole("button", { name: "Year" });
    expect(header(grid, "Year")).toHaveAttribute("aria-sort", "none");

    await user.click(year);
    expect(useLibraryStore.getState().movies.sort).toEqual([{ field: "year", descending: false }]);
    expect(sorted(grid)).toEqual(["Year:ascending"]);
    expect(header(grid, "Year").querySelector("svg")).not.toBeNull();
    expect(header(grid, "Title").querySelector("svg")).toBeNull();

    await user.click(year);
    expect(useLibraryStore.getState().movies.sort).toEqual([{ field: "year", descending: true }]);
    expect(sorted(grid)).toEqual(["Year:descending"]);

    await user.click(year);
    expect(useLibraryStore.getState().movies.sort).toEqual([{ field: "year", descending: false }]);
  });

  it("sorts every column; quantities and dates start with the biggest or newest", async () => {
    const { user, grid } = await toTable();
    useLibraryStore.getState().setColumnHidden("movies", "added", false);
    useLibraryStore.getState().setColumnHidden("movies", "modified", false);
    await waitFor(() => expect(header(grid, "Date added")).toBeInTheDocument());

    const expected: Record<string, { field: string; descending: boolean }> = {
      Year: { field: "year", descending: false },
      Quality: { field: "quality", descending: true },
      Size: { field: "total_file_size", descending: true },
      "Drive and folder": { field: "folder", descending: false },
      Rating: { field: "rating", descending: true },
      Runtime: { field: "runtime", descending: true },
      "Date added": { field: "date_added", descending: true },
      "File modified": { field: "file_modified", descending: true },
      Stage: { field: "stage", descending: false },
      Title: { field: "title", descending: false },
    };
    for (const [name, key] of Object.entries(expected)) {
      await user.click(within(grid).getByRole("button", { name }));
      expect(useLibraryStore.getState().movies.sort).toEqual([key]);
      expect(sorted(grid)).toEqual([`${name}:${key.descending ? "descending" : "ascending"}`]);
    }
  });

  it("remembers the sort and shows it in the grid's sort menu", async () => {
    const { user, grid } = await toTable();
    await user.click(within(grid).getByRole("button", { name: "Year" }));
    expect(savedLibrary().movies.sort).toEqual([{ field: "year", descending: false }]);
    await user.click(screen.getByRole("button", { name: "Grid view" }));
    expect(await screen.findByRole("button", { name: "Year, oldest first" })).toBeInTheDocument();
  });

  it("offers Date added and File modified in the grid's sort menu", async () => {
    const user = userEvent.setup();
    renderLibrary();
    await user.click(await screen.findByRole("button", { name: "Title A–Z" }));
    for (const name of [
      "Recently added",
      "Oldest added",
      "File modified, newest first",
      "File modified, oldest first",
    ]) {
      expect(await screen.findByRole("menuitemradio", { name })).toBeInTheDocument();
    }
    await user.click(screen.getByRole("menuitemradio", { name: "File modified, newest first" }));
    expect(useLibraryStore.getState().movies.sort).toEqual([
      { field: "file_modified", descending: true },
    ]);
    expect(sortChoiceOf(SORT_KEYS.modifiedDesc)).toBe("modifiedDesc");
    expect(SORT_CHOICES).toContain("addedAsc");
  });

  it("asks the backend for the whole library with the title as the final tie-break", async () => {
    const queries: LibraryQuery[] = [];
    const backend = createSyntheticBackend(30);
    setLibraryBackend({
      ...backend,
      rows: (query, offset, limit) => {
        queries.push(query);
        return backend.rows(query, offset, limit);
      },
    });
    const { user, grid } = await toTable();
    await user.click(within(grid).getByRole("button", { name: "Rating" }));
    await waitFor(() =>
      expect(queries.at(-1)?.sort).toEqual([
        { field: "rating", descending: true },
        { field: "title", descending: false },
      ]),
    );
  });
});

describe("the synthetic backend orders like the real one", () => {
  it("puts titles with no value last, whichever way a column is sorted", async () => {
    const backend = createSyntheticBackend(181);
    for (const field of ["rating", "file_modified", "quality"] as const) {
      for (const descending of [false, true]) {
        const window = await backend.rows(
          {
            kind: "movies",
            sort: [{ field, descending }],
            search: "",
            filters: [],
            rating_source: null,
          },
          0,
          181,
        );
        const known = window.items.map((row) =>
          field === "rating"
            ? row.rating !== null
            : field === "file_modified"
              ? row.file_modified_ms !== null
              : row.height > 0,
        );
        const firstGap = known.indexOf(false);
        if (firstGap !== -1) expect(known.slice(firstGap).includes(true)).toBe(false);
      }
    }
  });
});

describe("the Stage column", () => {
  it("has a visible Stage header that sorts, and labelled tags instead of a bare tick", async () => {
    const { user, grid } = await toTable();
    const stage = header(grid, "Stage");
    expect(stage).toHaveTextContent("Stage");
    await user.click(within(stage).getByRole("button", { name: "Stage" }));
    expect(useLibraryStore.getState().movies.sort).toEqual([{ field: "stage", descending: false }]);
    expect(sorted(grid)).toEqual(["Stage:ascending"]);
    const tags = Array.from(grid.querySelectorAll<HTMLElement>("[role=gridcell] [data-stage]"));
    expect(tags.length).toBeGreaterThan(5);
    const words = new Set(tags.map((tag) => tag.textContent));
    expect(["New", "Scraped", "Renamed"].some((word) => words.has(word))).toBe(true);
  });

  it("is information, not a control: an arrow cursor and the app tooltip, never a native title", async () => {
    const { grid } = await toTable();
    const tag = grid.querySelector<HTMLElement>("[role=gridcell] [data-stage]");
    expect(tag).not.toBeNull();
    expect(tag?.className).toContain("cursor-default");
    expect(grid.querySelectorAll("[role=gridcell] [title]")).toHaveLength(0);
  });

  it("keeps the warning colour for a title that needs attention", async () => {
    const { grid } = await toTable();
    const warn = grid.querySelector<HTMLElement>('[role=gridcell] [data-stage="attention"]');
    // The synthetic library has unmatched titles near the top of the list.
    expect(warn?.className ?? "").toContain("text-warn");
    expect(warn).toHaveTextContent(/Not matched|Uncertain|Files missing/);
  });
});

describe("the column chooser", () => {
  it("starts with the usual columns; Date added and File modified are off", async () => {
    const { grid } = await toTable();
    expect(
      within(grid)
        .getAllByRole("columnheader")
        .map((h) => h.getAttribute("aria-label")),
    ).toEqual([
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
    expect(SWITCHABLE_COLUMNS).not.toContain("title");
  });

  it("opens from a right-click on the header, leaves Title out and remembers the choice", async () => {
    const { user, grid } = await toTable();
    await user.pointer({ target: header(grid, "Year"), keys: "[MouseRight]" });
    const added = await screen.findByRole("menuitemcheckbox", { name: "Date added" });
    expect(added).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("menuitemcheckbox", { name: "Title" })).toBeNull();
    // The header's menu is the chooser, not the titles' actions.
    expect(screen.queryByRole("menuitem", { name: /^Rename/ })).toBeNull();

    await user.click(added);
    // The open menu hides the page from assistive technology, so look past that.
    await waitFor(() =>
      expect(
        within(grid).getByRole("columnheader", { name: "Date added", hidden: true }),
      ).toBeInTheDocument(),
    );
    // The menu stays open for the next column.
    await user.click(await screen.findByRole("menuitemcheckbox", { name: "Rating" }));
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(within(grid).queryByRole("columnheader", { name: "Rating" })).toBeNull(),
    );
    expect(savedLibrary().movies.hiddenColumns).toEqual(["modified", "rating"]);
  });

  it("shows the short date in the cell and the full date and time on hover", async () => {
    const { user, grid } = await toTable();
    useLibraryStore.getState().setColumnHidden("movies", "added", false);
    await waitFor(() => expect(header(grid, "Date added")).toBeInTheDocument());
    const cell = await waitFor(() => {
      const found = grid.querySelector<HTMLElement>(
        '[role="row"][data-index="0"] [role="gridcell"]:last-child span span',
      );
      if (found === null) throw new Error("no date yet");
      return found;
    });
    expect(cell.textContent).toMatch(/\d{4}/);
    await user.hover(cell);
    expect((await screen.findAllByText(/\d{1,2}:\d{2}/)).length).toBeGreaterThan(0);
  });

  it("hides the two new columns for a save made before they existed", async () => {
    window.localStorage.setItem(
      "cinevault.library.v1",
      JSON.stringify({ movies: { view: "table", hiddenColumns: ["stage"] } }),
    );
    vi.resetModules();
    const fresh = await import("./libraryStore");
    expect(fresh.useLibraryStore.getState().movies.hiddenColumns).toEqual([
      "stage",
      "added",
      "modified",
    ]);
    // A save that knows the columns is taken as it is.
    window.localStorage.setItem(
      "cinevault.library.v1",
      JSON.stringify({ columns: 2, movies: { hiddenColumns: [] } }),
    );
    vi.resetModules();
    const again = await import("./libraryStore");
    expect(again.useLibraryStore.getState().movies.hiddenColumns).toEqual([]);
  });
});
