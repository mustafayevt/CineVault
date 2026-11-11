import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setDetailBackend } from "../detail/detailBackend";
import type { MovieDetail } from "../detail/detailBackend";
import { SYNTHETIC_MOVIE_ID, syntheticMovieDetail } from "../detail/syntheticDetail";
import { setLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { createSyntheticLibraryOpsBackend } from "../library-ops/syntheticLibraryOps";
import { MoviePage } from "../movie/MoviePage";
import { setRenameHistoryBackend } from "../movie/renamedFrom";
import { setScanBackend, unavailableScanBackend } from "../scan/scanBackend";
import { ShowPage } from "../show/ShowPage";
import { restoreLayout, stubLayout } from "../../test/layout";
import { restorePlatform, stubPlatform } from "../../test/platform";
import { useToastStore } from "../toasts/toastStore";
import { setTvBackend } from "../tv/tvBackend";
import type { ShowDetail, TvBackend } from "../tv/tvBackend";
import {
  SYNTHETIC_SHOW_ID,
  createSyntheticTvBackend,
  syntheticShowDetail,
} from "../tv/syntheticTv";

function renderAt(path: string) {
  const root = createRootRoute();
  const stub = (name: string) => () => <p>{name}</p>;
  const routes = [
    createRoute({ getParentRoute: () => root, path: "/movies/$movieId", component: MoviePage }),
    createRoute({ getParentRoute: () => root, path: "/tv/$showId", component: ShowPage }),
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
    </QueryClientProvider>,
  );
  return router;
}

function movieWith(availability: string): MovieDetail {
  return { ...syntheticMovieDetail(), availability };
}

function showWith(availability: string): ShowDetail {
  return { ...syntheticShowDetail(), availability };
}

function useMovie(availability: string) {
  const base = {
    movie: vi.fn(() => Promise.resolve(movieWith(availability))),
    nfo: vi.fn(() => Promise.reject(new Error("no nfo"))),
    setWatched: vi.fn(() => Promise.resolve([])),
    restoreWatched: vi.fn(() => Promise.resolve()),
    castPortraits: () => Promise.resolve([]),
    personImdbId: () => Promise.resolve(null),
  };
  setDetailBackend(base);
}

function useShow(availability: string) {
  const backend: TvBackend = {
    ...createSyntheticTvBackend(),
    show: () => Promise.resolve(showWith(availability)),
  };
  setTvBackend(backend);
}

beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  setRenameHistoryBackend({
    renamedFrom: () => Promise.resolve(null),
    undo: () => Promise.resolve(),
  });
});

afterEach(() => {
  restoreLayout();
  restorePlatform();
  setDetailBackend(null);
  setTvBackend(null);
  setLibraryOpsBackend(null);
  setRenameHistoryBackend(null);
  setScanBackend(null);
  useToastStore.setState({ toasts: [], history: [] });
});

describe("the files notice on a title page", () => {
  it("offers Remove from library on a movie whose files are missing, and opens the Library after", async () => {
    useMovie("missing");
    const ops = createSyntheticLibraryOpsBackend();
    const removeFromLibrary = vi.spyOn(ops, "removeFromLibrary");
    setLibraryOpsBackend(ops);
    const router = renderAt(`/movies/${SYNTHETIC_MOVIE_ID}`);

    const notice = await screen.findByTestId("files-notice");
    expect(notice).toHaveTextContent("The files for this title are missing");
    expect(notice).toHaveTextContent("in case the drive is offline or the files were only moved");
    await userEvent.click(screen.getByTestId("files-notice-remove"));

    expect(removeFromLibrary).toHaveBeenCalledWith([SYNTHETIC_MOVIE_ID], false);
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(useToastStore.getState().toasts[0]?.message).toContain("from the library");
  });

  it("does not offer it when the drive is offline", async () => {
    useMovie("offline");
    renderAt(`/movies/${SYNTHETIC_MOVIE_ID}`);
    const notice = await screen.findByTestId("files-notice");
    expect(notice).toHaveTextContent("This title's library folder cannot be reached");
    expect(screen.queryByTestId("files-notice-remove")).toBeNull();
  });

  it("does not show a notice, or the button, for a title that is fine", async () => {
    useMovie("online");
    renderAt(`/movies/${SYNTHETIC_MOVIE_ID}`);
    await screen.findByRole("toolbar", { name: "Title actions" });
    expect(screen.queryByTestId("files-notice")).toBeNull();
  });

  it("stays on the page and keeps the title when the removal fails", async () => {
    useMovie("missing");
    const ops = createSyntheticLibraryOpsBackend();
    const removeFromLibrary = vi.fn(() =>
      Promise.resolve({
        removed: [],
        problems: [{ id: SYNTHETIC_MOVIE_ID, title: "x", reason: "The library is busy." }],
      }),
    );
    setLibraryOpsBackend({ ...ops, removeFromLibrary });
    const router = renderAt(`/movies/${SYNTHETIC_MOVIE_ID}`);
    await userEvent.click(await screen.findByTestId("files-notice-remove"));
    await waitFor(() => expect(removeFromLibrary).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(router.state.location.pathname).toBe(`/movies/${SYNTHETIC_MOVIE_ID}`);
  });

  it("offers it on a TV show whose files are missing, and not when its drive is offline", async () => {
    useShow("missing");
    const ops = createSyntheticLibraryOpsBackend();
    const removeFromLibrary = vi.spyOn(ops, "removeFromLibrary");
    setLibraryOpsBackend(ops);
    const router = renderAt(`/tv/${SYNTHETIC_SHOW_ID}`);
    await userEvent.click(await screen.findByTestId("files-notice-remove"));
    expect(removeFromLibrary).toHaveBeenCalledWith([SYNTHETIC_SHOW_ID], false);
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
  });

  it("leaves the TV show's offline notice without the button", async () => {
    useShow("offline");
    renderAt(`/tv/${SYNTHETIC_SHOW_ID}`);
    await screen.findByTestId("files-notice");
    expect(screen.queryByTestId("files-notice-remove")).toBeNull();
  });

  it.each(["missing", "offline"])(
    "offers Scan again for the movie's folder when its files are %s",
    async (availability) => {
      useMovie(availability);
      const scanTitle = vi.fn(() =>
        Promise.resolve({ job_id: 0, sources: 1, media_info: "ready" as const }),
      );
      setScanBackend({ ...unavailableScanBackend, available: true, scanTitle });
      renderAt(`/movies/${SYNTHETIC_MOVIE_ID}`);
      await userEvent.click(await screen.findByTestId("files-notice-scan"));
      expect(scanTitle).toHaveBeenCalledWith(SYNTHETIC_MOVIE_ID);
      await waitFor(() =>
        expect(useToastStore.getState().toasts[0]?.message).toMatch(/^Scanning /),
      );
    },
  );

  it("offers Scan again on a TV show whose files are missing", async () => {
    useShow("missing");
    const scanTitle = vi.fn(() =>
      Promise.resolve({ job_id: 0, sources: 1, media_info: "ready" as const }),
    );
    setScanBackend({ ...unavailableScanBackend, available: true, scanTitle });
    renderAt(`/tv/${SYNTHETIC_SHOW_ID}`);
    await userEvent.click(await screen.findByRole("button", { name: /^Scan the folder of / }));
    expect(scanTitle).toHaveBeenCalledWith(SYNTHETIC_SHOW_ID);
  });
});
