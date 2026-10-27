import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TitleBar } from "../shell/TitleBar";
import { setLibraryBackend } from "../library/libraryBackend";
import type {
  LibraryBackend,
  LibraryCard,
  LibraryKind,
  LibraryQuery,
  LibraryWindow,
  PersonHit,
} from "../library/libraryBackend";
import { highlightParts, highlightRanges, isSimilarOnly, matchScore } from "./highlight";
import {
  RECENT_LIMIT,
  pushOpenedTitle,
  pushRecentSearch,
  readOpenedTitles,
  readRecentSearches,
} from "./recents";

const card = (id: string, title: string, year: number, over: Partial<LibraryCard> = {}) =>
  ({
    id,
    title,
    original_title: "",
    year,
    is_new: false,
    needs_review: false,
    lifecycle: null,
    unmatched: false,
    watched: false,
    file_name: title,
    resolution: "1080p",
    hdr: null,
    poster: null,
    ...over,
  }) as LibraryCard;

const MOVIES = [
  card("m1", "Blade Runner", 1982),
  card("m2", "Blade Runner 2049", 2017),
  card("m3", "Bladerunners Anonymous", 2001),
  card("m4", "Heat", 1995),
];
const SHOWS = [card("s1", "Blade of Glory Show", 2010)];
const COLLECTIONS = [card("c1", "Blade Runner Collection", 0)];
const PEOPLE: PersonHit[] = [{ name: "Ridley Blade", role: "director", titles: 2 }];

interface FakeOptions {
  /** Called for every library query; may hold the answer back. */
  gate?: (kind: LibraryKind, search: string) => Promise<void>;
}

function fakeBackend(options: FakeOptions = {}) {
  const asked: { kind: LibraryKind; search: string }[] = [];
  const pool = { movies: MOVIES, shows: SHOWS, collections: COLLECTIONS } as const;
  const backend = {
    window: async (query: LibraryQuery, _offset: number, limit: number): Promise<LibraryWindow> => {
      asked.push({ kind: query.kind, search: query.search });
      await options.gate?.(query.kind, query.search);
      const words = query.search.toLowerCase().split(/\s+/).filter(Boolean);
      const items = pool[query.kind].filter((entry) =>
        words.every((word) => entry.title.toLowerCase().includes(word)),
      );
      return { total: items.length, offset: 0, items: items.slice(0, limit) };
    },
    rows: async () => ({ total: 0, offset: 0, items: [] }),
    counts: async () => ({ movies: 4, shows: 1, collections: 1 }),
    people: async (term: string) =>
      PEOPLE.filter((person) => person.name.toLowerCase().includes(term.toLowerCase())),
  } as unknown as LibraryBackend;
  return { backend, asked };
}

function renderBar(path = "/history") {
  const rootRoute = createRootRoute({
    component: () => <TitleBar platform="windows" windowApi={null} />,
  });
  const leaf = (to: string) =>
    createRoute({ getParentRoute: () => rootRoute, path: to, component: () => null });
  const router = createRouter({
    routeTree: rootRoute.addChildren(
      ["/library", "/history", "/movies/$movieId", "/tv/$showId"].map(leaf),
    ),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(<RouterProvider router={router} />);
  return router;
}

const field = () => screen.findByRole("combobox", { name: "Search library" });
const optionNames = (panel: HTMLElement) =>
  within(panel)
    .getAllByRole("option")
    .map((option) => option.textContent ?? "");

beforeEach(() => {
  window.localStorage.clear();
});
afterEach(() => {
  setLibraryBackend(null);
  vi.restoreAllMocks();
});

describe("the suggestion panel under the title-bar search", () => {
  it("drops grouped results under the field as you type, with a top hit and a See all row", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "blade runner");
    const panel = await screen.findByRole("listbox", { name: "Search suggestions" });
    await within(panel).findByText("Top hit");
    // The best fit leads and leaves its own group; groups without hits are not drawn.
    expect(within(panel).getAllByRole("group")).toHaveLength(3);
    expect(within(panel).getByText("Movies")).toBeVisible();
    expect(within(panel).getByText("Collections")).toBeVisible();
    expect(within(panel).queryByText("TV shows")).toBeNull();
    const names = optionNames(panel);
    expect(names[0]).toContain("Blade Runner");
    expect(names[0]).toContain("Movie · 1982");
    // People match the whole text only when it is in their name: none for two words here.
    expect(names.at(-1)).toBe("See all 4 results →");
    expect(screen.getByRole("combobox")).toHaveAttribute("aria-expanded", "true");
  });

  it("marks the typed letters in each title", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "run");
    const panel = await screen.findByRole("listbox");
    await within(panel).findByText("Top hit");
    const marks = Array.from(panel.querySelectorAll("mark")).map((mark) => mark.textContent);
    expect(marks.length).toBeGreaterThan(0);
    expect(new Set(marks.map((mark) => mark?.toLowerCase()))).toEqual(new Set(["run"]));
  });

  it("asks the library once for a burst of typing", async () => {
    const { backend, asked } = fakeBackend();
    setLibraryBackend(backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "blade");
    await screen.findByRole("listbox");
    expect(asked.filter((entry) => entry.kind === "movies").map((entry) => entry.search)).toEqual([
      "blade",
    ]);
  });

  it("keeps the last rows on screen while a newer search is out, then swaps them", async () => {
    let release: () => void = () => undefined;
    let hold = false;
    const { backend } = fakeBackend({
      gate: () => (hold ? new Promise<void>((resolve) => (release = resolve)) : Promise.resolve()),
    });
    setLibraryBackend(backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "heat");
    const panel = await screen.findByRole("listbox");
    await within(panel).findByText("Heat");
    hold = true;
    await user.type(screen.getByRole("combobox"), "x");
    // The new search is out; the old rows are still there, marked busy.
    await waitFor(() => expect(screen.getByRole("listbox")).toHaveAttribute("aria-busy", "true"));
    expect(within(screen.getByRole("listbox")).getByText("Heat")).toBeVisible();
    hold = false;
    await act(async () => release());
    await waitFor(() => expect(screen.getByRole("listbox")).toHaveAttribute("aria-busy", "false"));
    expect(screen.queryByText("Heat")).toBeNull();
  });

  it("shows the panel at once, without the grow animation, under reduced motion", async () => {
    setLibraryBackend(fakeBackend().backend);
    document.documentElement.dataset.motion = "reduced";
    try {
      const user = userEvent.setup();
      renderBar();
      await user.type(await field(), "heat");
      const panel = await screen.findByRole("listbox");
      expect(panel.style.animation).toBe("none");
      expect(panel.className).toContain("cv-pop");
    } finally {
      delete document.documentElement.dataset.motion;
    }
  });

  it("labels a result that only forgives a typo as similar", () => {
    expect(isSimilarOnly("Blade Runner", "blde")).toBe(true);
    expect(isSimilarOnly("Blade Runner", "run")).toBe(false);
  });
});

describe("keys in the suggestion panel", () => {
  it("moves the highlight with the arrows and wraps at both ends", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    renderBar();
    const input = await field();
    await user.type(input, "blade runner");
    const panel = await screen.findByRole("listbox");
    await within(panel).findByText("Top hit");
    const options = within(panel).getAllByRole("option");
    expect(input).not.toHaveAttribute("aria-activedescendant");
    await user.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute("aria-activedescendant", options[0]?.id);
    expect(options[0]).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute("aria-activedescendant", options[1]?.id);
    await user.keyboard("{ArrowUp}{ArrowUp}");
    // Up from the first row wraps to the last, the See all row.
    expect(input).toHaveAttribute("aria-activedescendant", options.at(-1)?.id);
    await user.keyboard("{ArrowDown}");
    expect(input).toHaveAttribute("aria-activedescendant", options[0]?.id);
  });

  it("opens the highlighted title with Enter, and remembers it as opened", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    const router = renderBar();
    await user.type(await field(), "heat");
    await screen.findByText("Top hit");
    await user.keyboard("{ArrowDown}{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/movies/m4"));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(readOpenedTitles().map((entry) => entry.title)).toEqual(["Heat"]);
  });

  it("opens a title with a click", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    const router = renderBar();
    await user.type(await field(), "glory");
    const option = await screen.findByRole("option", { name: /Blade of Glory Show/ });
    await user.click(option);
    await waitFor(() => expect(router.state.location.pathname).toBe("/tv/s1"));
  });

  it("goes to the Library search with Enter when nothing is highlighted", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    const router = renderBar();
    await user.type(await field(), "blade{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(router.state.location.search).toEqual({ q: "blade" });
    expect(readRecentSearches()).toEqual(["blade"]);
  });

  it("goes to the Library search from the See all row", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    const router = renderBar();
    await user.type(await field(), "blade");
    await user.click(await screen.findByRole("option", { name: /See all \d+ results/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(router.state.location.search).toEqual({ q: "blade" });
  });

  it("searches a person's credits from the People group", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    const router = renderBar();
    await user.type(await field(), "ridley");
    await user.click(await screen.findByRole("option", { name: /Ridley Blade/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(router.state.location.search).toEqual({ q: 'director:"Ridley Blade"' });
  });

  it("closes on the first Esc, keeping the text, and clears on the second", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    renderBar();
    const input = await field();
    await user.type(input, "heat");
    await screen.findByRole("listbox");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(input).toHaveValue("heat");
    await user.keyboard("{Escape}");
    expect(input).toHaveValue("");
  });

  it("closes when the pointer goes down outside the field and the panel", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "heat");
    await screen.findByRole("listbox");
    await user.click(document.body);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("says when nothing matches and offers no See all row", async () => {
    setLibraryBackend(fakeBackend().backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "zzzz");
    expect(await screen.findByText("No matches for “zzzz”")).toBeVisible();
    expect(screen.queryByRole("option")).toBeNull();
  });

  it("leaves a text with a filter in it to the Library and only offers to go there", async () => {
    const { backend, asked } = fakeBackend();
    setLibraryBackend(backend);
    const user = userEvent.setup();
    renderBar();
    await user.type(await field(), "year:2010..2019");
    const option = await screen.findByRole("option");
    expect(option).toHaveTextContent("Search the library for “year:2010..2019” →");
    expect(asked).toHaveLength(0);
  });
});

describe("recent searches and recently opened titles", () => {
  it("lists them when the empty field is focused, and a click runs the search", async () => {
    setLibraryBackend(fakeBackend().backend);
    pushRecentSearch("heat");
    pushRecentSearch("blade runner");
    pushOpenedTitle({ kind: "movie", id: "m4", title: "Heat", year: 1995, poster: null });
    const user = userEvent.setup();
    const router = renderBar();
    await user.click(await field());
    const panel = await screen.findByRole("listbox");
    expect(within(panel).getByText("Recent searches")).toBeVisible();
    expect(within(panel).getByText("Recently opened")).toBeVisible();
    expect(optionNames(panel)).toEqual(["blade runner", "heat", "HeatMovie · 1995"]);
    await user.click(within(panel).getByRole("option", { name: "heat" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(router.state.location.search).toEqual({ q: "heat" });
  });

  it("clears each list with its Clear button", async () => {
    setLibraryBackend(fakeBackend().backend);
    pushRecentSearch("heat");
    pushOpenedTitle({ kind: "movie", id: "m4", title: "Heat", year: 1995, poster: null });
    const user = userEvent.setup();
    renderBar();
    await user.click(await field());
    const panel = await screen.findByRole("listbox");
    const clears = within(panel).getAllByRole("button", { name: "Clear" });
    expect(clears).toHaveLength(2);
    await user.click(clears[0] as HTMLElement);
    expect(readRecentSearches()).toEqual([]);
    expect(readOpenedTitles()).toHaveLength(1);
    await user.click(within(screen.getByRole("listbox")).getByRole("button", { name: "Clear" }));
    expect(readOpenedTitles()).toEqual([]);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("keeps six of each, newest first, once each", () => {
    for (let index = 0; index < 9; index += 1) pushRecentSearch(`search ${index}`);
    pushRecentSearch("SEARCH 5");
    const searches = readRecentSearches();
    expect(searches).toHaveLength(RECENT_LIMIT);
    expect(searches[0]).toBe("SEARCH 5");
    expect(searches.filter((entry) => entry.toLowerCase() === "search 5")).toHaveLength(1);
    window.localStorage.setItem("cinevault.search.recent", "{broken");
    expect(readRecentSearches()).toEqual([]);
  });
});

describe("marking and ranking matches", () => {
  it("marks words ignoring case and accents, merging touching hits", () => {
    expect(highlightParts("Amélie Poulain", "amelie")).toEqual([
      { text: "Amélie", hit: true },
      { text: " Poulain", hit: false },
    ]);
    expect(highlightRanges("Blade Runner", "ru bl")).toEqual([
      { start: 0, end: 2 },
      { start: 6, end: 8 },
    ]);
    expect(highlightRanges("Heat", "")).toEqual([]);
  });

  it("scores an exact title above a prefix, a word, a contained text and a typo", () => {
    expect(matchScore("Heat", "heat")).toBe(100);
    expect(matchScore("Heat Wave", "heat")).toBe(80);
    expect(matchScore("The Heat", "heat")).toBe(60);
    expect(matchScore("Overheated", "heat")).toBe(40);
    expect(matchScore("Hat", "heat")).toBe(0);
  });
});
