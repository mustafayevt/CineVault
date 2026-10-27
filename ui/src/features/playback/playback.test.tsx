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

import { restoreLayout, stubLayout } from "../../test/layout";
import { createSyntheticDetailBackend, SYNTHETIC_MOVIE_ID } from "../detail/syntheticDetail";
import { setDetailBackend } from "../detail/detailBackend";
import { MoviePage } from "../movie/MoviePage";
import { ShowPage } from "../show/ShowPage";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { SYNTHETIC_SHOW_ID, createSyntheticTvBackend } from "../tv/syntheticTv";
import { setTvBackend } from "../tv/tvBackend";
import { PlaybackError, setPlaybackBackend } from "./playbackBackend";
import type { PlaybackBackend, PlayOutcome } from "./playbackBackend";

function backendOf(
  outcome: PlayOutcome = { player: "mpv", parts: 2, opened: 2 },
): PlaybackBackend & { play: ReturnType<typeof vi.fn> } {
  return { play: vi.fn(() => Promise.resolve(outcome)) };
}

function renderRoute(path: string, pattern: string, component: () => React.JSX.Element): void {
  const rootRoute = createRootRoute();
  const route = createRoute({ getParentRoute: () => rootRoute, path: pattern, component });
  const router = createRouter({
    routeTree: rootRoute.addChildren([route]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const renderMovie = (): void =>
  renderRoute(`/movies/${SYNTHETIC_MOVIE_ID}`, "/movies/$movieId", MoviePage);

const renderShow = (): void => renderRoute(`/tv/${SYNTHETIC_SHOW_ID}`, "/tv/$showId", ShowPage);

beforeEach(() => {
  stubLayout();
  useToastStore.setState({ toasts: [] });
  useNotificationStore.getState().clear();
  setDetailBackend(createSyntheticDetailBackend());
  setTvBackend(createSyntheticTvBackend());
});

afterEach(() => {
  restoreLayout();
  setDetailBackend(null);
  setTvBackend(null);
  setPlaybackBackend(null);
});

describe("the Play action", () => {
  it("plays the movie from the hero button", async () => {
    const backend = backendOf();
    setPlaybackBackend(backend);
    const user = userEvent.setup();
    renderMovie();
    await user.click(await screen.findByRole("button", { name: "Play" }));
    await waitFor(() => expect(backend.play).toHaveBeenCalledWith(SYNTHETIC_MOVIE_ID));
    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(useNotificationStore.getState().items).toHaveLength(0);
  });

  it("plays the episode from the button of its row", async () => {
    const backend = backendOf({ player: "vlc", parts: 1, opened: 1 });
    setPlaybackBackend(backend);
    const user = userEvent.setup();
    renderShow();
    const [row] = await screen.findAllByTestId("episode-row");
    await user.click(within(row as HTMLElement).getByRole("button"));
    const panel = await screen.findByTestId("episode-panel");
    await user.click(within(panel).getByRole("button", { name: "Play" }));
    await waitFor(() => expect(backend.play).toHaveBeenCalledTimes(1));
  });

  it("plays the next episode of a show from the header button", async () => {
    const backend = backendOf({ player: "vlc", parts: 1, opened: 1 });
    setPlaybackBackend(backend);
    const user = userEvent.setup();
    renderShow();
    const bar = await screen.findByRole("toolbar", { name: "Show actions" });
    await user.click(within(bar).getByRole("button", { name: "Play" }));
    await waitFor(() => expect(backend.play).toHaveBeenCalledTimes(1));
  });

  it("says plainly why a title could not be played", async () => {
    const backend = backendOf();
    backend.play.mockRejectedValueOnce(
      new PlaybackError("player_not_found", "The player mpv.exe was not found."),
    );
    setPlaybackBackend(backend);
    const user = userEvent.setup();
    renderMovie();
    await user.click(await screen.findByRole("button", { name: "Play" }));
    await waitFor(() => expect(useNotificationStore.getState().items).toHaveLength(1));
    const [note] = useNotificationStore.getState().items;
    expect(note).toMatchObject({
      severity: "error",
      detail: "The player mpv.exe was not found.",
    });
    expect(note?.title).toMatch(/^Could not play /);
  });

  it("tells when the system player was given only the first part", async () => {
    setPlaybackBackend(backendOf({ player: "system", parts: 2, opened: 1 }));
    const user = userEvent.setup();
    renderMovie();
    await user.click(await screen.findByRole("button", { name: "Play" }));
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(useToastStore.getState().toasts[0]?.message).toMatch(/first of 2 parts/);
  });
});
