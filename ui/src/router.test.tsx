import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory, createRouter } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { router as appRouter } from "./router";

// The real route tree on a memory history.
function open(path: string) {
  const router = createRouter({
    routeTree: appRouter.routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
    defaultNotFoundComponent: appRouter.options.defaultNotFoundComponent,
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

describe("the routes of the new design", () => {
  it("registers every page of the app", () => {
    const paths = new Set(Object.keys(appRouter.routesByPath));
    for (const path of [
      "/",
      "/library",
      "/movies/$movieId",
      "/movies/$movieId/edit",
      "/tv/$showId",
      "/tv/$showId/edit",
      "/scrape",
      "/scrape/match/$itemId",
      "/artwork",
      "/rename",
      "/drives",
      "/torrents",
      "/history",
      "/settings",
      "/welcome",
      "/gallery",
    ]) {
      expect(paths, path).toContain(path);
    }
  });

  it("shows the History page at /history", async () => {
    open("/history");
    expect(await screen.findByRole("heading", { name: "History", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("group", { name: "Show" })).toBeInTheDocument();
  });

  it("shows the first-run setup on its own at /welcome, without the shell", async () => {
    open("/welcome");
    expect(
      await screen.findByRole("heading", { name: "Where are your movies and shows?", level: 1 }),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("app-frame")).toBeNull();
  });

  it("starts on the Library", async () => {
    const router = open("/");
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
  });
});
