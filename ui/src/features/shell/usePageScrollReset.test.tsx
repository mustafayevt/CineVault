import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { usePageScrollReset } from "./usePageScrollReset";

function Shell() {
  const main = usePageScrollReset<HTMLElement>();
  return (
    <main ref={main} data-testid="main">
      <Outlet />
    </main>
  );
}

function setup() {
  const root = createRootRoute({ component: Shell });
  const page = (path: string) =>
    createRoute({ getParentRoute: () => root, path, component: () => <h1>{path}</h1> });
  const router = createRouter({
    routeTree: root.addChildren([page("/tv/$showId"), page("/tv/$showId/season/$seasonNumber")]),
    history: createMemoryHistory({ initialEntries: ["/tv/1"] }),
  });
  render(<RouterProvider router={router} />);
  return router;
}

describe("a new page", () => {
  it("opens at its top, while a change of the address search keeps the place", async () => {
    const router = setup();
    const main = await screen.findByTestId("main");
    main.scrollTop = 1028;

    await act(() => router.navigate({ href: "/tv/1?season=2" }));
    expect(main.scrollTop).toBe(1028);

    await act(() => router.navigate({ href: "/tv/1/season/1" }));
    expect(
      await screen.findByRole("heading", { name: "/tv/$showId/season/$seasonNumber" }),
    ).toBeInTheDocument();
    expect(main.scrollTop).toBe(0);

    main.scrollTop = 373;
    await act(() => router.navigate({ href: "/tv/1/season/2" }));
    expect(main.scrollTop).toBe(0);
  });
});

describe("going back", () => {
  it("returns to where the page was left, and a new page still opens at its top", async () => {
    const router = setup();
    const main = await screen.findByTestId("main");
    main.scrollTop = 640;
    main.dispatchEvent(new Event("scroll"));

    await act(() => router.navigate({ href: "/tv/1/season/1" }));
    expect(main.scrollTop).toBe(0);

    await act(async () => {
      router.history.back();
    });
    expect(await screen.findByRole("heading", { name: "/tv/$showId" })).toBeInTheDocument();
    expect(main.scrollTop).toBe(640);

    // A page opened by a click is new, so it opens at its top again.
    await act(() => router.navigate({ href: "/tv/1/season/2" }));
    expect(main.scrollTop).toBe(0);
  });
});
