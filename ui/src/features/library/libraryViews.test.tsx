import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { validateLibrarySearch } from "../../router";
import { restoreLayout, stubLayout } from "../../test/layout";
import { LibraryPage } from "./LibraryPage";
import { setLibraryBackend } from "./libraryBackend";
import { resetLibraryStore, useLibraryStore } from "./libraryStore";
import { createSyntheticBackend, syntheticRow } from "./syntheticLibrary";
import { cardTitle } from "./titleFacts";

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
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId",
        component: () => <p>Movie page</p>,
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

const tile = async (index: number) => {
  const cell = await waitFor(() => {
    const found = document.querySelector<HTMLElement>(`[data-index="${index}"][data-id]`);
    if (found === null) throw new Error(`no poster ${index}`);
    return found;
  });
  const button = cell.querySelector("button");
  if (button === null) throw new Error("no button");
  return button;
};
const bar = () => screen.queryByRole("toolbar", { name: "Bulk actions" });

describe("the poster grid", () => {
  it("selects on click, adds with Ctrl, extends with Shift and selects all with Ctrl+A", async () => {
    const user = userEvent.setup();
    renderLibrary();
    await user.click(await tile(1));
    expect(await within(bar() as HTMLElement).findByText("1 selected")).toBeInTheDocument();
    await user.keyboard("{Control>}");
    await user.click(await tile(3));
    await user.keyboard("{/Control}");
    expect(within(bar() as HTMLElement).getByText("2 selected")).toBeInTheDocument();
    await user.keyboard("{Shift>}");
    await user.click(await tile(6));
    await user.keyboard("{/Shift}");
    // The Shift range runs from the anchor (poster 3) to poster 6.
    expect(within(bar() as HTMLElement).getByText("4 selected")).toBeInTheDocument();
    (await tile(0)).closest<HTMLElement>("[role='group']")?.focus();
    await user.keyboard("{Control>}a{/Control}");
    expect(await within(bar() as HTMLElement).findByText("181 selected")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(bar()).toBeNull());
  });

  it("marks a selected poster and opens a title on double-click", async () => {
    const user = userEvent.setup();
    const router = renderLibrary();
    const first = await tile(2);
    await user.click(first);
    expect(first).toHaveAttribute("aria-pressed", "true");
    await user.dblClick(first);
    await waitFor(() => expect(router.state.location.pathname).toMatch(/^\/movies\/\d+/));
    expect(await screen.findByText("Movie page")).toBeInTheDocument();
  });

  it("opens the current title with Enter after moving with the arrow keys", async () => {
    const user = userEvent.setup();
    const router = renderLibrary();
    await user.click(await tile(0));
    await user.keyboard("{ArrowRight}{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toMatch(/^\/movies\/\d+/));
  });

  it("shows the right-click menu of the selection, with counts, on a poster", async () => {
    const user = userEvent.setup();
    renderLibrary();
    await user.click(await tile(0));
    await user.keyboard("{Shift>}");
    await user.click(await tile(3));
    await user.keyboard("{/Shift}");
    await user.pointer({ target: await tile(2), keys: "[MouseRight]" });
    expect(await screen.findByRole("menuitem", { name: /^Rename 4 titles/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: /^Scrape 4 titles/ })).toBeInTheDocument();
  });

  it("selects only the poster that is right-clicked when it is not selected", async () => {
    const user = userEvent.setup();
    renderLibrary();
    await user.click(await tile(0));
    await user.pointer({ target: await tile(5), keys: "[MouseRight]" });
    await screen.findByRole("menuitem", { name: /^Rename/ });
    expect(within(bar() as HTMLElement).getByText("1 selected")).toBeInTheDocument();
    expect(await tile(5)).toHaveAttribute("aria-pressed", "true");
    expect(await tile(0)).toHaveAttribute("aria-pressed", "false");
  });
});

describe("the table", () => {
  async function toTable() {
    const user = userEvent.setup();
    renderLibrary();
    await user.click(await screen.findByRole("button", { name: "Table view" }));
    await screen.findByRole("grid", { name: "Movies table" });
    return user;
  }
  const rowAt = (index: number) =>
    waitFor(() => {
      const found = document.querySelector<HTMLElement>(`[role="row"][data-index="${index}"]`);
      if (found === null) throw new Error(`no row ${index}`);
      return found;
    });

  it("shows Title, Year, Quality, Size, Drive and folder, Rating and Runtime, with the Stage column after the checkbox", async () => {
    await toTable();
    const grid = screen.getByRole("grid", { name: "Movies table" });
    const headers = within(grid)
      .getAllByRole("columnheader")
      .map((header) => header.getAttribute("aria-label"));
    expect(headers).toEqual([
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

  it("removes the Stage column from the Columns menu and remembers it", async () => {
    const user = await toTable();
    await user.click(screen.getByRole("button", { name: "Columns" }));
    await user.click(await screen.findByRole("menuitemcheckbox", { name: "Stage" }));
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(
        within(screen.getByRole("grid", { name: "Movies table" })).queryByRole("columnheader", {
          name: "Stage",
        }),
      ).not.toBeInTheDocument(),
    );
    expect(useLibraryStore.getState().movies.hiddenColumns).toContain("stage");
  });

  it("selects rows with click, Ctrl, Shift and Ctrl+A, and opens one on double-click", async () => {
    const user = await toTable();
    await user.click(await rowAt(1));
    expect(await within(bar() as HTMLElement).findByText("1 selected")).toBeInTheDocument();
    await user.keyboard("{Control>}");
    await user.click(await rowAt(4));
    await user.keyboard("{/Control}");
    expect(within(bar() as HTMLElement).getByText("2 selected")).toBeInTheDocument();
    await user.keyboard("{Shift>}");
    await user.click(await rowAt(7));
    await user.keyboard("{/Shift}");
    expect(within(bar() as HTMLElement).getByText("4 selected")).toBeInTheDocument();
    screen.getByRole("grid", { name: "Movies table" }).focus();
    await user.keyboard("{Control>}a{/Control}");
    expect(await within(bar() as HTMLElement).findByText("181 selected")).toBeInTheDocument();
  });

  it("opens a title on double-click", async () => {
    const user = await toTable();
    await user.dblClick(await rowAt(2));
    expect(await screen.findByText("Movie page")).toBeInTheDocument();
  });

  it("sorts by a header and keeps the sort when the view switches back to the grid", async () => {
    const user = await toTable();
    await user.click(within(screen.getByRole("grid")).getByRole("button", { name: /Year/ }));
    expect(useLibraryStore.getState().movies.sort).toEqual([{ field: "year", descending: false }]);
    await user.click(screen.getByRole("button", { name: "Grid view" }));
    expect(await screen.findByRole("button", { name: "Year, oldest first" })).toBeInTheDocument();
  });

  it("keeps the selection when the view switches", async () => {
    const user = await toTable();
    await user.click(await rowAt(1));
    await within(bar() as HTMLElement).findByText("1 selected");
    await user.click(screen.getByRole("button", { name: "Grid view" }));
    expect(within(bar() as HTMLElement).getByText("1 selected")).toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: "Escape" });
  });
});

describe("the title of a card", () => {
  it("uses the original title only when asked and when there is one", () => {
    const card = {
      ...syntheticRow(0).card,
      original_title: "千と千尋の神隠し",
      title: "Spirited Away",
    };
    expect(cardTitle(card, false)).toBe("Spirited Away");
    expect(cardTitle(card, true)).toBe("千と千尋の神隠し");
    expect(cardTitle({ ...card, original_title: "" }, true)).toBe("Spirited Away");
  });

  it("reads the parsed title of an unmatched card and falls back to the file name", () => {
    const card = syntheticRow(0).card;
    expect(cardTitle({ ...card, unmatched: true, title: "The Batman" })).toBe("The Batman");
    expect(
      cardTitle({ ...card, unmatched: true, title: " ", file_name: "the.batman.2022.mkv" }),
    ).toBe("the.batman.2022");
  });
});
