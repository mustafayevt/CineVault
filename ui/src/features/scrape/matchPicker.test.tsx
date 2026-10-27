import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { restorePlatform, stubPlatform } from "../../test/platform";
import { handleEscape } from "../shell/escStack";
import { setLibraryBackend } from "../library/libraryBackend";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { useToastStore } from "../toasts/toastStore";
import { MatchPickerPage } from "./MatchPickerPage";
import { setReviewBackend } from "./reviewBackend";
import { hitOfCandidate, setScrapeBackend } from "./scrapeBackend";
import { createSyntheticReviewBackend } from "./syntheticReview";
import { createSyntheticScrapeBackend } from "./syntheticScrape";

function renderPicker(id = "1") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/scrape/match/$itemId",
        component: MatchPickerPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        component: () => <p>Library page</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/library", `/scrape/match/${id}`] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

let backend: ReturnType<typeof createSyntheticScrapeBackend>;

beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
  // The title the item is linked to, scored like the search results.
  const current = hitOfCandidate(
    {
      provider: "tmdb",
      providerId: "693134",
      title: "Dune: Part Two",
      originalTitle: null,
      year: 2024,
      director: "Denis Villeneuve",
      runtimeMinutes: 166,
      language: null,
      overview: null,
      posterKey: null,
      confidence: 0.93,
      rank: 0.93,
      evidence: [],
      breakdown: { similarity: 0.97, yearPenalty: 0, posterPenalty: 0, evidence: 0 },
    },
    "current",
  );
  backend = createSyntheticScrapeBackend({ current: () => current });
  setScrapeBackend(backend);
  setReviewBackend(createSyntheticReviewBackend([]));
  setLibraryBackend(createSyntheticBackend(10));
});
afterEach(() => {
  restoreLayout();
  restorePlatform();
  setScrapeBackend(null);
  setReviewBackend(null);
  setLibraryBackend(null);
});

const searchBox = () =>
  screen.findByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" });
const candidates = () => screen.findByRole("listbox", { name: "Candidates" });

describe("the match picker", () => {
  it("is a page with a header naming the title, Cancel with its Esc hint and the file evidence", async () => {
    renderPicker();
    expect(
      await screen.findByRole("heading", { name: "Scrape · Dune: Part Two (2024)" }),
    ).toBeInTheDocument();
    const cancel = screen.getAllByRole("button", { name: /^Cancel/ })[0] as HTMLElement;
    expect(cancel).toHaveTextContent("Esc");
    expect(screen.queryByRole("dialog")).toBeNull();
    // The search field has the focus on opening, and holds the title.
    const box = await searchBox();
    await waitFor(() => expect(box).toHaveFocus());
    expect(box).toHaveValue("Dune: Part Two");
    expect(screen.getByRole("group", { name: "Kind" })).toBeInTheDocument();
  });

  it("pins the current match first with its tag, lists the other results, and chooses the best other", async () => {
    renderPicker();
    const list = await candidates();
    await waitFor(() => expect(within(list).getAllByRole("option").length).toBeGreaterThan(2));
    const options = within(list).getAllByRole("option");
    expect(options[0]).toHaveTextContent("Dune: Part Two");
    expect(options[0]).toHaveTextContent("Current");
    expect(options[0]).toHaveAttribute("aria-selected", "false");
    expect(options[1]).toHaveAttribute("aria-selected", "true");
    expect(within(list).getByText("Current match")).toBeInTheDocument();
    expect(within(list).getByText(/more results? from/i)).toBeInTheDocument();
  });

  it("compares Now and After for the chosen result and warns about a different runtime", async () => {
    renderPicker();
    const card = await screen.findByRole("region", { name: "What will change" });
    await waitFor(() => expect(within(card).getByText("After")).toBeInTheDocument());
    expect(within(card).getByText("Now")).toBeInTheDocument();
    expect(within(card).getByText("Dune: Part Two (2024)")).toBeInTheDocument();
    expect(within(card).getByText(/Replaced \(old files to Recycle Bin\)/)).toBeInTheDocument();
    expect(within(card).getByText(/at next Rename/)).toBeInTheDocument();
    expect(
      within(card).getByText(/^Runtime differs from your file by \d+ minutes$/),
    ).toBeInTheDocument();
  });

  it("says the Trash for replaced files on Linux", async () => {
    stubPlatform("linux");
    renderPicker();
    const card = await screen.findByRole("region", { name: "What will change" });
    await waitFor(() => expect(within(card).getByText("After")).toBeInTheDocument());
    expect(within(card).getByText(/Replaced \(old files to Trash\)/)).toBeInTheDocument();
  });

  it("takes a pasted TMDB link as the one exact result and chooses it", async () => {
    const user = userEvent.setup();
    renderPicker();
    const box = await searchBox();
    await user.clear(box);
    await user.paste("https://www.themoviedb.org/movie/949");
    const list = await candidates();
    await waitFor(() => {
      const heat = within(list)
        .getAllByRole("option")
        .filter((o) => /^Heat/.test(o.textContent ?? ""));
      expect(heat).toHaveLength(1);
      expect(heat[0]).toHaveAttribute("aria-selected", "true");
    });
    const card = screen.getByRole("region", { name: "What will change" });
    expect(within(card).getAllByText("Heat (1995)").length).toBeGreaterThan(0);
  });

  it("applies the chosen result with Scrape with this, closes the picker and offers Undo", async () => {
    const user = userEvent.setup();
    const router = renderPicker();
    await screen.findByRole("region", { name: "What will change" });
    await user.click(await screen.findByRole("button", { name: /^Scrape with this/ }));
    await waitFor(() => expect(backend.applied).toHaveLength(1));
    expect(backend.applied[0]?.target.itemId).toBe("1");
    expect(backend.applied[0]?.request.provider).toBeDefined();
    // Back at the page the picker was opened from.
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    const toast = useToastStore.getState().toasts.find((entry) => /^Scraped /.test(entry.message));
    expect(toast?.undo).toBeDefined();
    await act(async () => {
      await toast?.undo?.();
    });
    expect(backend.undone).toHaveLength(1);
  });

  it("applies with Enter in the search field, and a click alone writes nothing", async () => {
    const user = userEvent.setup();
    renderPicker();
    const list = await candidates();
    await waitFor(() => expect(within(list).getAllByRole("option").length).toBeGreaterThan(2));
    await user.click(within(list).getAllByRole("option")[2] as HTMLElement);
    expect(backend.applied).toHaveLength(0);
    expect(within(list).getAllByRole("option")[2]).toHaveAttribute("aria-selected", "true");
    await user.click(await searchBox());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(backend.applied).toHaveLength(1));
  });

  it("scrapes again with the same match when the current match is chosen, and says nothing else changes", async () => {
    const user = userEvent.setup();
    renderPicker();
    const list = await candidates();
    await waitFor(() => expect(within(list).getAllByRole("option").length).toBeGreaterThan(1));
    await user.click(within(list).getAllByRole("option")[0] as HTMLElement);
    const card = screen.getByRole("region", { name: "What will change" });
    expect(within(card).getByText(/This is the current match/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^Scrape with this/ }));
    await waitFor(() => expect(backend.applied).toHaveLength(1));
  });

  it("closes with Esc and with Cancel, back to where it was opened, and writes nothing", async () => {
    const user = userEvent.setup();
    const router = renderPicker();
    await searchBox();
    await act(async () => {
      expect(handleEscape({ defaultPrevented: false, target: document.body })).toBe(true);
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(backend.applied).toHaveLength(0);

    await act(async () => router.history.forward());
    await user.click((await screen.findAllByRole("button", { name: /^Cancel/ }))[0] as HTMLElement);
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(backend.applied).toHaveLength(0);
  });

  it("says when the title is not in the library", async () => {
    renderPicker("nope");
    expect(await screen.findByText("This title could not be found")).toBeInTheDocument();
  });
});
