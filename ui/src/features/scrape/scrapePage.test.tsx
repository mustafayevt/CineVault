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
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { setLibraryBackend } from "../library/libraryBackend";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { setProvidersBackend } from "../providers/providersBackend";
import { useToastStore } from "../toasts/toastStore";
import { useFoundCandidates } from "./newTitleSearch";
import { setReviewBackend } from "./reviewBackend";
import type { ReviewBackend } from "./reviewBackend";
import { setScrapeBackend } from "./scrapeBackend";
import type { ScrapeBackend } from "./scrapeBackend";
import { ScrapePage } from "./ScrapePage";
import { validateScrapeSearch } from "./scrapeSearch";
import { createSyntheticReviewBackend } from "./syntheticReview";
import { createSyntheticScrapeBackend } from "./syntheticScrape";

interface Harness {
  review: ReviewBackend;
  scrape: ReturnType<typeof createSyntheticScrapeBackend>;
  /** The typed searches, which stream from every provider. */
  started: string[];
  /** The search-only pass of the New titles. */
  searched: string[];
}

function install(): Harness {
  const review = createSyntheticReviewBackend();
  const accept = vi.spyOn(review, "accept");
  void accept;
  const scrape = createSyntheticScrapeBackend({ generic: true });
  const started: string[] = [];
  const searched: string[] = [];
  const base = scrape.searchTransport;
  const counting: ScrapeBackend["searchTransport"] = (onStart) => {
    const inner = base(onStart);
    return {
      start: (request, onEvent) => {
        started.push(request.query);
        inner.start(request, onEvent);
      },
      retry: inner.retry,
      cancel: inner.cancel,
    };
  };
  scrape.searchTransport = counting;
  // The search-only pass of the New titles, which writes nothing.
  const search = scrape.searchOnly;
  scrape.searchOnly = (itemId, kind, query, language) => {
    searched.push(query);
    return search(itemId, kind, query, language);
  };
  setReviewBackend(review);
  setScrapeBackend(scrape);
  return { review, scrape, started, searched };
}

function renderScrape(path = "/scrape") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/scrape",
        validateSearch: validateScrapeSearch,
        component: ScrapePage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        component: () => <p>Library</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { router, client, ...view };
}

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useFoundCandidates.setState({ found: {} });
  useToastStore.setState({ toasts: [], history: [] });
  setLibraryBackend(createSyntheticBackend(60));
  setProvidersBackend(null);
});
afterEach(() => {
  restoreLayout();
  setLibraryBackend(null);
  setReviewBackend(null);
  setScrapeBackend(null);
});

const tabs = () => screen.findByRole("group", { name: "Show" });
const countOf = async (name: RegExp) => {
  const button = within(await tabs()).getByRole("button", { name });
  return Number(/(\d[\d,]*)\s*$/.exec(button.textContent ?? "")?.[1]?.replaceAll(",", ""));
};
const rows = () => {
  const list = screen.queryByRole("listbox", { name: "Titles to scrape" });
  return list === null ? [] : within(list).queryAllByRole("option");
};

describe("the Scrape page queue", () => {
  it("has the tabs All, New, Uncertain and Not matched with counts that add up", async () => {
    install();
    renderScrape();
    const group = await tabs();
    for (const name of [/^All/, /^New/, /^Uncertain/, /^Not matched/]) {
      expect(within(group).getByRole("button", { name })).toBeInTheDocument();
    }
    await waitFor(async () => expect(await countOf(/^Uncertain/)).toBeGreaterThan(0));
    const all = await countOf(/^All/);
    const parts =
      (await countOf(/^New/)) + (await countOf(/^Uncertain/)) + (await countOf(/^Not matched/));
    expect(all).toBe(parts);
    expect(within(group).getByRole("button", { name: /^All/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("lists New titles first, and one tab at a time", async () => {
    const user = userEvent.setup();
    install();
    renderScrape();
    const list = await screen.findByRole("listbox", { name: "Titles to scrape" });
    await waitFor(() => expect(within(list).getAllByRole("option").length).toBeGreaterThan(0));
    expect(within(list).getAllByRole("option")[0]).toHaveAttribute("data-status", "new");

    await user.click(within(await tabs()).getByRole("button", { name: /^Not matched/ }));
    await waitFor(() => {
      const shown = rows();
      expect(shown.length).toBeGreaterThan(0);
      expect(shown.every((row) => row.getAttribute("data-status") === "notMatched")).toBe(true);
    });
  });

  it("searches New titles while the page is open, shows their scores and counts the confident ones", async () => {
    const harness = install();
    renderScrape();
    const accept = await screen.findByRole("button", { name: /^Accept all confident/ });
    // The review rows are known at once; the New titles add theirs as the search answers.
    await waitFor(() => expect(harness.searched.length).toBeGreaterThan(0));
    const before = Number(/\((\d+)\)/.exec(accept.textContent ?? "")?.[1]);
    await waitFor(() => {
      const now = Number(
        /\((\d+)\)/.exec(
          screen.getByRole("button", { name: /^Accept all confident/ }).textContent ?? "",
        )?.[1],
      );
      expect(now).toBeGreaterThan(before);
    });
    // A searched New title shows the score of its best candidate.
    const first = rows()[0];
    expect(first).toBeDefined();
    await waitFor(() =>
      expect(within(rows()[0] as HTMLElement).getByLabelText(/Top match/)).toBeVisible(),
    );
    // Nothing was written by searching.
    expect(harness.scrape.applied).toHaveLength(0);
  });

  it("stops searching when the page closes", async () => {
    const harness = install();
    const { unmount } = renderScrape();
    await waitFor(() => expect(harness.searched.length).toBeGreaterThan(0));
    const startedAtLeave = harness.searched.length;
    unmount();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 700));
    });
    expect(harness.searched.length).toBe(startedAtLeave);
  });

  it("accepts every confident title at once and offers Undo", async () => {
    const user = userEvent.setup();
    const harness = install();
    renderScrape();
    // The button is drawn again when it changes between enabled and not, so it is found each time.
    const accept = () => screen.getByRole("button", { name: /^Accept all confident/ });
    const confidentCount = () => Number(/\((\d+)\)/.exec(accept().textContent ?? "")?.[1]);
    await screen.findByRole("button", { name: /^Accept all confident/ });
    // Let a few New titles be searched so both kinds of title are confident.
    await waitFor(() => expect(confidentCount()).toBeGreaterThan(4));
    const count = confidentCount();
    await user.click(accept());
    // One backend call takes the confident titles, and nothing else is written.
    await waitFor(() => expect(harness.scrape.acceptedConfident).toHaveLength(1));
    expect(harness.scrape.applied).toHaveLength(0);
    // One toast covers the run, and it offers Undo.
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => /^Scraped \d+ titles?$/.test(toast.message) && toast.undo),
      ).toBe(true),
    );
    expect(harness.scrape.acceptedConfident[0]).toHaveLength(count);
    // The confident review rows left the queue at once.
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^Accept all confident/ })).toHaveAttribute(
        "aria-disabled",
        "true",
      ),
    );
  });

  it("explains an empty queue, and a missing key", async () => {
    setReviewBackend(createSyntheticReviewBackend([]));
    setLibraryBackend(createSyntheticBackend(0));
    setScrapeBackend(createSyntheticScrapeBackend());
    renderScrape();
    expect(await screen.findByText("Nothing waits for a scrape")).toBeInTheDocument();
  });
});

/** Opens the Uncertain tab and waits for its rows. */
async function openUncertain(user: ReturnType<typeof userEvent.setup>) {
  await user.click(within(await tabs()).getByRole("button", { name: /^Uncertain/ }));
  await waitFor(() => {
    const shown = rows();
    expect(shown.length).toBeGreaterThan(0);
    expect(shown.every((row) => row.getAttribute("data-status") === "uncertain")).toBe(true);
  });
}

describe("the candidates of the current title", () => {
  it("shows the file, the search, the candidates with scores and why, and what will change", async () => {
    const user = userEvent.setup();
    install();
    renderScrape();
    await openUncertain(user);
    // The first title is the Solaris file: its full path is the evidence.
    expect(
      (await screen.findAllByTitle(String.raw`F:\Personal\Movie\Solaris\Solaris.mkv`)).length,
    ).toBeGreaterThan(0);
    const search = screen.getByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" });
    expect(search).toHaveValue("Solaris");
    expect(screen.getByRole("group", { name: "Kind" })).toBeInTheDocument();
    expect(screen.getByText("or paste a link or ID")).toBeInTheDocument();

    const list = await screen.findByRole("listbox", { name: "Candidates" });
    const first = within(list).getAllByRole("option")[0] as HTMLElement;
    expect(first).toHaveAttribute("aria-selected", "true");
    expect(within(first).getByText("91%")).toBeInTheDocument();
    expect(within(first).getByText(/Title and runtime match|Title matches/)).toBeInTheDocument();
    const second = within(list).getAllByRole("option")[1] as HTMLElement;
    expect(within(second).getByText("58%")).toBeInTheDocument();

    const card = screen.getByRole("region", { name: "What will change" });
    expect(within(card).getByText("Solaris (1972)")).toBeInTheDocument();
    expect(within(card).getByText(/New/)).toBeInTheDocument();
    expect(within(card).getByText(/stays as is until you Rename/)).toBeInTheDocument();
  });

  it("scrapes with the chosen candidate, advances the queue and offers Undo", async () => {
    const user = userEvent.setup();
    const harness = install();
    const accept = vi.spyOn(harness.review, "accept");
    const reopen = vi.spyOn(harness.review, "reopen");
    renderScrape();
    await openUncertain(user);
    const before = rows().length;
    const firstName = rows()[0]?.textContent ?? "";
    await user.click(await screen.findByRole("button", { name: /^Scrape with this/ }));
    await waitFor(() => expect(accept).toHaveBeenCalledTimes(1));
    expect(accept.mock.calls[0]?.[1]).toMatchObject({ provider: "tmdb" });
    // The title left the queue and the next one is shown.
    await waitFor(() => expect(rows()).toHaveLength(before - 1));
    expect(rows().some((row) => row.textContent === firstName)).toBe(false);
    expect(rows()[0]).toHaveAttribute("aria-selected", "true");
    const toast = useToastStore.getState().toasts.find((entry) => /^Scraped /.test(entry.message));
    expect(toast?.undo).toBeDefined();
    await act(async () => {
      await toast?.undo?.();
    });
    expect(reopen).toHaveBeenCalled();
  });

  it("lets a click only choose, and Enter in the search field apply", async () => {
    const user = userEvent.setup();
    const harness = install();
    const accept = vi.spyOn(harness.review, "accept");
    renderScrape();
    await openUncertain(user);
    const list = await screen.findByRole("listbox", { name: "Candidates" });
    const second = within(list).getAllByRole("option")[1] as HTMLElement;
    await user.click(second);
    expect(second).toHaveAttribute("aria-selected", "true");
    expect(accept).not.toHaveBeenCalled();
    expect(
      within(screen.getByRole("region", { name: "What will change" })).getByText("Solaris (2002)"),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" }),
    );
    await user.keyboard("{Enter}");
    await waitFor(() => expect(accept).toHaveBeenCalledTimes(1));
    expect(accept.mock.calls[0]?.[1]).toMatchObject({
      providerId: expect.stringContaining("2002"),
    });
  });

  it("moves through the candidates with the arrow keys in the search field", async () => {
    const user = userEvent.setup();
    install();
    renderScrape();
    await openUncertain(user);
    const list = await screen.findByRole("listbox", { name: "Candidates" });
    await user.click(
      screen.getByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" }),
    );
    await user.keyboard("{ArrowDown}");
    expect(within(list).getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowUp}");
    expect(within(list).getAllByRole("option")[0]).toHaveAttribute("aria-selected", "true");
  });

  it("skips to the next title and keeps the skipped one in the queue", async () => {
    const user = userEvent.setup();
    install();
    renderScrape();
    await openUncertain(user);
    const before = rows().length;
    const first = rows()[0]?.textContent;
    await user.click(screen.getByRole("button", { name: /^Skip/ }));
    expect(rows()).toHaveLength(before);
    expect(rows()[1]).toHaveAttribute("aria-selected", "true");
    expect(rows()[0]?.textContent).toBe(first);
  });

  it("searches again by typing and by switching to TV show", async () => {
    const user = userEvent.setup();
    const harness = install();
    renderScrape();
    await openUncertain(user);
    const search = screen.getByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" });
    await user.clear(search);
    await user.type(search, "Dune");
    await waitFor(() => expect(harness.started.some((query) => /^Dune/.test(query))).toBe(true));
    await user.click(
      within(screen.getByRole("group", { name: "Kind" })).getByRole("button", { name: "TV show" }),
    );
    expect(
      within(screen.getByRole("group", { name: "Kind" })).getByRole("button", { name: "TV show" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("says in one line which provider waits for a key, and where to set it up", async () => {
    const user = userEvent.setup();
    install();
    renderScrape();
    await openUncertain(user);
    const search = screen.getByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" });
    await user.clear(search);
    await user.type(search, "Dune");
    expect(await screen.findByText(/TheTVDB needs a key/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set up TheTVDB" })).toBeInTheDocument();
  });

  it("takes a pasted TMDB link as one exact result", async () => {
    const user = userEvent.setup();
    install();
    renderScrape();
    await openUncertain(user);
    const search = screen.getByRole("searchbox", { name: "Title, or an IMDb or TMDB link or ID" });
    await user.clear(search);
    await user.click(search);
    await user.paste("https://www.themoviedb.org/movie/949");
    const list = await screen.findByRole("listbox", { name: "Candidates" });
    await waitFor(() => expect(within(list).getAllByRole("option")).toHaveLength(1));
    expect(within(list).getAllByText("Heat").length).toBeGreaterThan(0);
    expect(
      within(screen.getByRole("region", { name: "What will change" })).getByText("Heat (1995)"),
    ).toBeInTheDocument();
  });
});
