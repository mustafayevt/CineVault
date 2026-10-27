import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { handleEscape } from "../shell/escStack";
import { restoreLayout, stubLayout } from "../../test/layout";
import { setTvBackend } from "../tv/tvBackend";
import { SYNTHETIC_SHOW_ID, syntheticShowDetail } from "../tv/syntheticTv";
import { ShowPage } from "./ShowPage";
import { distinctSize, playEpisode, rowsOf, showProgress } from "./showModel";

function renderShow(path = `/tv/${SYNTHETIC_SHOW_ID}`) {
  const root = createRootRoute();
  const stub = (name: string) => () => <p>{name}</p>;
  const routes = [
    createRoute({ getParentRoute: () => root, path: "/tv/$showId", component: ShowPage }),
    createRoute({ getParentRoute: () => root, path: "/tv/$showId/edit", component: stub("Edit") }),
    createRoute({
      getParentRoute: () => root,
      path: "/scrape/match/$itemId",
      component: stub("Match"),
    }),
    createRoute({ getParentRoute: () => root, path: "/rename", component: stub("Rename") }),
    createRoute({ getParentRoute: () => root, path: "/artwork", component: stub("Artwork") }),
    createRoute({ getParentRoute: () => root, path: "/library", component: stub("Library") }),
  ];
  const router = createRouter({
    routeTree: root.addChildren(routes),
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

beforeEach(() => stubLayout());
afterEach(() => {
  restoreLayout();
  setTvBackend(null);
});

describe("the show model", () => {
  const detail = syntheticShowDetail();

  it("counts what is on disk against what has aired, specials left out", () => {
    const progress = showProgress(detail);
    expect(progress.owned).toBeGreaterThan(0);
    expect(progress.total).toBeGreaterThanOrEqual(progress.owned);
  });

  it("counts a file that holds two episodes once", () => {
    const [first] = detail.episodes;
    expect(first).toBeDefined();
    const row = first as NonNullable<typeof first>;
    const twin = { ...row, id: "x", number: row.number + 50 };
    expect(distinctSize([row, twin])).toBe(distinctSize([row]));
  });

  it("lists the missing episodes of a season among its rows, in order", () => {
    const numbers = rowsOf(detail, 2).map((row) => row.number);
    expect(numbers).toEqual([...numbers].sort((a, b) => a - b));
    expect(rowsOf(detail, 2).some((row) => row.kind === "missing")).toBe(true);
  });

  it("plays the first unwatched episode that has a file", () => {
    const next = playEpisode(detail);
    expect(next).not.toBeNull();
    expect(next?.file_name).not.toBe("");
  });
});

describe("the TV show page", () => {
  it("has the same header actions as the Movie page", async () => {
    renderShow();
    const bar = await screen.findByRole("toolbar", { name: "Show actions" });
    const names = within(bar)
      .getAllByRole("button")
      .map((button) => button.textContent);
    expect(names.slice(0, 6)).toEqual([
      "Play",
      "Scrape",
      "Rename",
      "Move",
      "Open folder",
      "Change artwork",
    ]);
    expect(within(bar).getByRole("button", { name: /More actions/ })).toBeInTheDocument();
  });

  it("has the floating poster and backdrop buttons", async () => {
    const user = userEvent.setup();
    const router = renderShow();
    await user.click(await screen.findByRole("button", { name: "Change poster" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(router.state.location.searchStr).toMatch(/type="?poster/);
  });

  it("opens the picker on the fanart from the backdrop button and from the header button without a type", async () => {
    const user = userEvent.setup();
    const router = renderShow();
    await user.click(await screen.findByRole("button", { name: "Change backdrop" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(router.state.location.searchStr).toMatch(/type="?fanart/);
    await router.navigate({ to: "/tv/$showId", params: { showId: SYNTHETIC_SHOW_ID } });
    const bar = await screen.findByRole("toolbar", { name: "Show actions" });
    await user.click(within(bar).getByRole("button", { name: "Change artwork" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(router.state.location.searchStr).not.toContain("type=");
  });

  it("shows the show facts and one section per season, specials last", async () => {
    renderShow();
    const facts = await screen.findByTestId("show-facts");
    expect(facts).toHaveTextContent(/of \d+ episodes/);
    const sections = document.querySelectorAll("section[data-season]");
    const numbers = [...sections].map((section) => Number(section.getAttribute("data-season")));
    expect(numbers.length).toBeGreaterThan(1);
    expect(numbers[numbers.length - 1]).toBe(0);
    expect(screen.getAllByText(/\d+ missing/)[0]).toHaveClass("text-warn");
  });

  it("opens an episode in place, keeps only one open, and closes it with Escape", async () => {
    const user = userEvent.setup();
    renderShow();
    const rows = await screen.findAllByTestId("episode-row");
    const first = within(rows[0] as HTMLElement).getByRole("button");
    await user.click(first);
    expect(await screen.findByTestId("episode-panel")).toBeInTheDocument();
    expect(first).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByTestId("episode-panel");
    expect(within(panel).getByRole("button", { name: "Scrape episode" })).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: /All streams/ })).toBeInTheDocument();

    const second = within(rows[1] as HTMLElement).getByRole("button");
    await user.click(second);
    await waitFor(() => expect(first).toHaveAttribute("aria-expanded", "false"));
    expect(second).toHaveAttribute("aria-expanded", "true");
    expect(document.querySelectorAll("[data-testid=episode-panel]").length).toBeLessThanOrEqual(2);

    act(() => {
      handleEscape({ defaultPrevented: false, target: document.body });
    });
    await waitFor(() => expect(second).toHaveAttribute("aria-expanded", "false"));
  });

  it("toggles an episode with Enter and Space on its title, turning the chevron with it", async () => {
    const user = userEvent.setup();
    renderShow();
    const rows = await screen.findAllByTestId("episode-row");
    const row = rows[0] as HTMLElement;
    const toggle = within(row).getByRole("button");
    const chevron = () => toggle.querySelector("svg");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(chevron()?.getAttribute("class")).not.toContain("rotate-90");
    toggle.focus();
    await user.keyboard("{Enter}");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(chevron()?.getAttribute("class")).toContain("rotate-90");
    // The panel opens through the height track: it is flagged open once it has mounted closed.
    const track = await waitFor(() => {
      const found = row.querySelector<HTMLElement>(".cv-expand");
      expect(found).not.toBeNull();
      expect(found).toHaveAttribute("data-open", "true");
      return found as HTMLElement;
    });
    expect(row).toHaveAttribute("data-state", "open");
    await user.keyboard(" ");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(chevron()?.getAttribute("class")).not.toContain("rotate-90");
    // Closing keeps the panel until its height has animated away, then it leaves.
    expect(track).toBeInTheDocument();
    await waitFor(() => expect(row.querySelector(".cv-expand")).toBeNull());
  });

  it("animates the chevron and the card's tint with the panel, and not at all under reduced motion", async () => {
    const user = userEvent.setup();
    renderShow();
    const rows = await screen.findAllByTestId("episode-row");
    const row = rows[0] as HTMLElement;
    expect(row.className).toContain("duration-(--dur-expand)");
    expect(row.className).toContain("transition-[background-color,box-shadow]");
    document.documentElement.dataset.motion = "reduced";
    try {
      cleanup();
      renderShow();
      const again = (await screen.findAllByTestId("episode-row"))[0] as HTMLElement;
      expect(again.className).not.toContain("transition-");
      const toggle = within(again).getAllByRole("button")[0] as HTMLElement;
      await user.click(toggle);
      expect(toggle.querySelector("svg")?.getAttribute("class")).not.toContain("transition-");
    } finally {
      delete document.documentElement.dataset.motion;
    }
  });

  it("lists an episode the library lacks as a muted row, not an error", async () => {
    renderShow();
    await screen.findAllByTestId("episode-row");
    const missing = screen.getAllByText("Missing")[0] as HTMLElement;
    expect(missing).toHaveClass("text-warn");
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
