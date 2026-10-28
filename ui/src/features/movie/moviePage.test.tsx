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

import { DeleteConfirmDialog } from "../delete/DeleteConfirmDialog";
import { DetailRequestError, setDetailBackend } from "../detail/detailBackend";
import type { MovieDetail, MovieDetailBackend, NfoPreview } from "../detail/detailBackend";
import {
  SYNTHETIC_MOVIE_ID,
  syntheticMovieDetail,
  syntheticNfo,
  syntheticStaleNfo,
} from "../detail/syntheticDetail";
import { setOpenFolderBackend } from "../detail/openFolderBackend";
import { setScanBackend, unavailableScanBackend } from "../scan/scanBackend";
import { restoreLayout, stubLayout } from "../../test/layout";
import { restorePlatform, stubPlatform } from "../../test/platform";
import { useSettingsStore } from "../settings/settingsStore";
import { useMoveToStore } from "../move-to/moveToStore";
import { createSyntheticHistoryBackend } from "../history/syntheticHistory";
import { setHistoryBackend } from "../history/historyBackend";
import { useToastStore } from "../toasts/toastStore";
import { MoviePage } from "./MoviePage";
import { setRenameHistoryBackend } from "./renamedFrom";

function backendOf(detail: MovieDetail, nfo: NfoPreview = syntheticNfo()): MovieDetailBackend {
  return {
    movie: vi.fn(() => Promise.resolve(detail)),
    nfo: vi.fn(() => Promise.resolve(nfo)),
    setWatched: vi.fn(() => Promise.resolve([])),
    restoreWatched: vi.fn(() => Promise.resolve()),
    castPortraits: () => Promise.resolve([]),
    personImdbId: () => Promise.resolve(null),
  };
}

interface Rendered {
  router: { state: { location: { pathname: string; searchStr: string } } };
}

/** The Movie page with the routes it links to, wired like the app. */
function renderMovie(path = `/movies/${SYNTHETIC_MOVIE_ID}`): Rendered {
  const root = createRootRoute();
  const stub = (name: string) => () => <p>{name}</p>;
  const routes = [
    createRoute({ getParentRoute: () => root, path: "/movies/$movieId", component: MoviePage }),
    createRoute({
      getParentRoute: () => root,
      path: "/movies/$movieId/edit",
      component: stub("Edit page"),
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/scrape/match/$itemId",
      component: stub("Match page"),
    }),
    createRoute({ getParentRoute: () => root, path: "/rename", component: stub("Rename page") }),
    createRoute({ getParentRoute: () => root, path: "/artwork", component: stub("Artwork page") }),
    createRoute({ getParentRoute: () => root, path: "/library", component: stub("Library page") }),
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
      <DeleteConfirmDialog />
    </QueryClientProvider>,
  );
  return { router };
}

beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  setDetailBackend(backendOf(syntheticMovieDetail()));
  setRenameHistoryBackend({
    renamedFrom: () => Promise.resolve(null),
    undo: () => Promise.resolve(),
  });
});

afterEach(() => {
  restoreLayout();
  restorePlatform();
  setDetailBackend(null);
  setRenameHistoryBackend(null);
  setHistoryBackend(null);
  useToastStore.setState({ toasts: [], history: [] });
  setOpenFolderBackend(null);
  setScanBackend(null);
});

describe("the Movie page header", () => {
  it("lists Play, Scrape, Rename, Move, Open folder and Change artwork in order, with a menu after them", async () => {
    renderMovie();
    const bar = await screen.findByRole("toolbar", { name: "Title actions" });
    const names = within(bar)
      .getAllByRole("button")
      .map((button) => button.textContent ?? button.getAttribute("aria-label"));
    expect(names).toEqual([
      "Play",
      "Scrape",
      "Rename",
      "Move",
      "Open folder",
      "Change artwork",
      "",
    ]);
    expect(within(bar).getByRole("button", { name: /More actions/ })).toBeInTheDocument();
  });

  it("asks first from Delete… in the ⋯ menu, then sends the title to the Recycle Bin, answers with an Undo toast and returns to the Library", async () => {
    const history = createSyntheticHistoryBackend();
    const deleteTitles = vi.spyOn(history, "deleteTitles");
    setHistoryBackend(history);
    const { router } = renderMovie();
    const bar = await screen.findByRole("toolbar", { name: "Title actions" });
    await userEvent.click(within(bar).getByRole("button", { name: /More actions/ }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete…" }));
    const dialog = await screen.findByRole("dialog", { name: /^Delete “.*”\?$/ });
    expect(deleteTitles).not.toHaveBeenCalled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(deleteTitles).toHaveBeenCalledWith([SYNTHETIC_MOVIE_ID]);
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    const toast = useToastStore.getState().toasts.at(-1);
    expect(toast?.message).toMatch(/^Deleted .* to the Recycle Bin$/);
    expect(toast?.undo).toBeDefined();
  });

  it("opens the Move to menu from Move", async () => {
    const user = userEvent.setup();
    renderMovie();
    const bar = await screen.findByRole("toolbar", { name: "Title actions" });
    const move = within(bar).getByRole("button", { name: "Move" });
    expect(move).not.toHaveAttribute("aria-disabled", "true");
    await user.click(move);
    expect(useMoveToStore.getState().request?.ids).toEqual([SYNTHETIC_MOVIE_ID]);
  });

  it("opens the match picker from Scrape and the rename page from Rename", async () => {
    const user = userEvent.setup();
    const { router } = renderMovie();
    await user.click(await screen.findByRole("button", { name: "Scrape" }));
    await waitFor(() =>
      expect(router.state.location.pathname).toBe(`/scrape/match/${SYNTHETIC_MOVIE_ID}`),
    );
  });

  it("opens Edit details and Show NFO from the ⋯ menu", async () => {
    const user = userEvent.setup();
    const { router } = renderMovie();
    await user.click(await screen.findByRole("button", { name: /More actions/ }));
    expect(screen.getByRole("menuitem", { name: "Rewrite NFO" })).toBeInTheDocument();
    expect(
      screen.getByRole("menuitem", { name: "Remove from library (keep files)" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Ignore this folder" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Show NFO" }));
    expect(await screen.findByText(/<movie>/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /More actions/ }));
    await user.click(screen.getByRole("menuitem", { name: "Edit details" }));
    await waitFor(() =>
      expect(router.state.location.pathname).toBe(`/movies/${SYNTHETIC_MOVIE_ID}/edit`),
    );
  });
});

describe("the Movie page artwork buttons", () => {
  it("opens the picker on the poster from the poster's floating button, reachable by keyboard", async () => {
    const user = userEvent.setup();
    const { router } = renderMovie();
    const button = await screen.findByRole("button", { name: "Change poster" });
    expect(screen.getByTestId("title-poster")).toContainElement(button);
    await user.hover(screen.getByTestId("title-poster"));
    button.focus();
    expect(button).toHaveFocus();
    await user.click(button);
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(decodeURIComponent(router.state.location.searchStr)).toMatch(
      new RegExp(`ids="?${SYNTHETIC_MOVIE_ID}"?`),
    );
    expect(router.state.location.searchStr).toMatch(/type="?poster/);
  });

  it("opens the picker on the fanart from the backdrop's floating button", async () => {
    const user = userEvent.setup();
    const { router } = renderMovie();
    await user.click(await screen.findByRole("button", { name: "Change backdrop" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(router.state.location.searchStr).toMatch(/type="?fanart/);
  });

  it("opens the picker with no type from the header's Change artwork button, and the ⋯ menu has no Artwork… item", async () => {
    const user = userEvent.setup();
    const { router } = renderMovie();
    const bar = await screen.findByRole("toolbar", { name: "Title actions" });
    await user.click(within(bar).getByRole("button", { name: "Change artwork" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(decodeURIComponent(router.state.location.searchStr)).toMatch(
      new RegExp(`ids="?${SYNTHETIC_MOVIE_ID}"?`),
    );
    expect(router.state.location.searchStr).not.toContain("type=");
  });

  it("ends the ⋯ menu with Delete… in the danger colour, after a divider", async () => {
    const user = userEvent.setup();
    renderMovie();
    await user.click(await screen.findByRole("button", { name: /More actions/ }));
    const items = screen.getAllByRole("menuitem");
    expect(items.at(-1)).toHaveTextContent("Delete…");
    expect(items.at(-1)?.className).toContain("text-danger");
    expect(screen.queryByRole("menuitem", { name: "Artwork…" })).toBeNull();
    expect(items.slice(0, 4).map((item) => item.textContent)).toEqual([
      "Edit details",
      "Show NFO",
      "Rewrite NFO",
      "Reload media info",
    ]);
  });
});

describe("the Movie page title", () => {
  it("heads the page with the original title when the setting is on, the library title otherwise", async () => {
    const detail = {
      ...syntheticMovieDetail(),
      title: "Spirited Away",
      original_title: "千と千尋の神隠し",
    };
    setDetailBackend(backendOf(detail));
    renderMovie();
    expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("Spirited Away");
  });

  it("uses the original title when providers.prefer-original-title is on", async () => {
    useSettingsStore.setState({ values: { "providers.prefer-original-title": true } });
    const detail = {
      ...syntheticMovieDetail(),
      title: "Spirited Away",
      original_title: "千と千尋の神隠し",
    };
    setDetailBackend(backendOf(detail));
    renderMovie();
    expect(await screen.findByRole("heading", { level: 1 })).toHaveTextContent("千と千尋の神隠し");
  });
});

describe("the Movie page body", () => {
  it("shows the plot, the facts and the first cast, with All N expanding the rest in place", async () => {
    const user = userEvent.setup();
    renderMovie();
    expect(await screen.findByText(/Paul Atreides joins Chani/)).toHaveClass("cv-plot");
    const director = document.querySelector("[data-fact=director]");
    expect(director).toHaveTextContent("Denis Villeneuve");
    expect(document.querySelector("[data-fact=released]")).toHaveTextContent("Mar 1, 2024");
    const toggle = screen.getByRole("button", { name: /^All \d+$/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(screen.getByRole("button", { name: "Show less" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  it("counts the artwork types and links an empty one to the picker on that type", async () => {
    const user = userEvent.setup();
    const { router } = renderMovie();
    expect(await screen.findByText(/\d of 8 types/)).toBeInTheDocument();
    const missing = document.querySelector("[data-state=missing]");
    expect(missing).not.toBeNull();
    await user.click(missing as HTMLElement);
    await waitFor(() => expect(router.state.location.pathname).toBe("/artwork"));
    expect(router.state.location.searchStr).toContain("type=");
  });

  it("explains a movie that is gone", async () => {
    setDetailBackend({
      ...backendOf(syntheticMovieDetail()),
      movie: () => Promise.reject(new DetailRequestError("not_found", "gone")),
    });
    renderMovie("/movies/999");
    expect(await screen.findByRole("heading", { name: "That movie is gone" })).toBeInTheDocument();
  });
});

describe("the Technical section", () => {
  it("summarises video, audio, subtitles, file and NFO state", async () => {
    renderMovie();
    await screen.findByText("Technical");
    expect(document.querySelector("[data-spec=video]")).toHaveTextContent("2160p");
    expect(document.querySelector("[data-spec=video]")).toHaveTextContent(
      "Dolby Vision Profile 8.1 · HDR10",
    );
    expect(document.querySelector("[data-spec=audio]")).toHaveTextContent("TrueHD");
    expect(document.querySelector("[data-spec=subtitles]")).toHaveTextContent("English");
    expect(document.querySelector("[data-spec=file]")).toHaveTextContent("Matroska");
    expect(await screen.findByText("up to date")).toBeInTheDocument();
  });

  it("says when the NFO differs from the library", async () => {
    setDetailBackend(backendOf(syntheticMovieDetail(), syntheticStaleNfo()));
    renderMovie();
    expect(await screen.findByText("differs from the library")).toBeInTheDocument();
  });

  it("opens every stream and the file list in place, and hides them again", async () => {
    const user = userEvent.setup();
    renderMovie();
    const show = await screen.findByRole("button", { name: /Show all streams and details/ });
    expect(show).toHaveAttribute("aria-expanded", "false");
    await user.click(show);
    const details = await screen.findByTestId("technical-details");
    expect(within(details).getByText("Container")).toBeInTheDocument();
    for (const heading of ["Video", "Audio", "Subtitles", "Files in folder"]) {
      expect(within(details).getAllByText(heading).length).toBeGreaterThan(0);
    }
    expect(within(details).getByRole("button", { name: "Copy as text" })).toBeInTheDocument();
    // The HDR section: the profile, the mastering display and the light levels.
    const hdr = within(details).getByTestId("hdr-details");
    expect(hdr).toHaveTextContent("Profile 8.1 · Base layer HDR10");
    expect(hdr).toHaveTextContent("Display P3 · 0.005 to 1,000 cd/m²");
    expect(hdr).toHaveTextContent("MaxCLL");
    expect(hdr).toHaveTextContent("MaxFALL");
    await user.click(within(details).getByRole("button", { name: /Hide details/ }));
    expect(screen.getByRole("button", { name: /Show all streams and details/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("offers Read now when the video details were never read", async () => {
    const detail = syntheticMovieDetail();
    setDetailBackend(
      backendOf({
        ...detail,
        videos: detail.videos.map((video) => ({ ...video, analysed: false, streams: [] })),
      }),
    );
    renderMovie();
    expect(await screen.findByRole("button", { name: "Read now" })).toBeInTheDocument();
  });

  it("says MediaInfo is missing and offers to download it when Read now cannot run", async () => {
    const detail = syntheticMovieDetail();
    setDetailBackend(
      backendOf({
        ...detail,
        videos: detail.videos.map((video) => ({ ...video, analysed: false, streams: [] })),
      }),
    );
    setScanBackend({
      ...unavailableScanBackend,
      readPendingMediaInfo: () => Promise.resolve("missing"),
    });
    const user = userEvent.setup();
    renderMovie();
    await user.click(await screen.findByRole("button", { name: "Read now" }));
    expect(await screen.findByRole("button", { name: "Download MediaInfo" })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(/MediaInfo/);
  });

  it("shows where the title was renamed from and undoes only that title", async () => {
    const undo = vi.fn(() => Promise.resolve());
    setRenameHistoryBackend({
      renamedFrom: () =>
        Promise.resolve({ from: "Dune.Part.Two.2024.2160p", atMs: Date.now() - 60_000 }),
      undo,
    });
    const user = userEvent.setup();
    renderMovie();
    expect(await screen.findByText("Dune.Part.Two.2024.2160p")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(undo).toHaveBeenCalledWith(SYNTHETIC_MOVIE_ID));
  });
});
