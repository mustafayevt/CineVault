import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Link,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { useShellStore } from "../../stores/shell";
import { AppShell } from "./AppShell";

/** A window whose media queries answer as listed; anything else does not match. */
function stubMedia(matching: readonly string[]) {
  window.matchMedia = ((query: string) => ({
    matches: matching.includes(query),
    media: query,
    onchange: null,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

const original = window.matchMedia;

beforeEach(() => {
  useShellStore.setState({ navSheetOpen: false });
});
afterEach(() => {
  window.matchMedia = original;
  delete document.documentElement.dataset.touch;
});

function renderShell(path = "/library") {
  const rootRoute = createRootRoute({
    component: () => <AppShell platform="windows" windowApi={null} />,
  });
  const routes = [
    "/library",
    "/scrape",
    "/rename",
    "/drives",
    "/torrents",
    "/history",
    "/settings",
  ].map((to) =>
    createRoute({
      getParentRoute: () => rootRoute,
      path: to,
      component: () => (
        <>
          <h1>{`Page ${to}`}</h1>
          <Link to="/scrape">Go to Scrape</Link>
        </>
      ),
    }),
  );
  const router = createRouter({
    routeTree: rootRoute.addChildren(routes),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

describe("the compact layout of a phone or tablet", () => {
  it("keeps the full layout on a wide window", async () => {
    stubMedia([]);
    renderShell();
    expect(await screen.findByRole("navigation", { name: "Main" })).toBeVisible();
    expect(screen.getByTestId("app-frame")).toHaveAttribute("data-layout", "full");
    expect(screen.queryByRole("button", { name: "Menu" })).toBeNull();
  });

  it("hides the sidebar behind a menu button that opens it as a drawer", async () => {
    const user = userEvent.setup();
    stubMedia(["(max-width: 1023.98px)"]);
    const router = renderShell();
    await screen.findByRole("heading", { name: "Page /library" });
    expect(screen.getByTestId("app-frame")).toHaveAttribute("data-layout", "compact");
    expect(screen.queryByRole("navigation", { name: "Main" })).toBeNull();
    expect(screen.getByRole("button", { name: "Back" })).toBeInTheDocument();
    // The search fills the bar without the keyboard hint.
    expect(screen.getByRole("search").textContent).not.toContain("Ctrl");

    const menu = screen.getByRole("button", { name: "Menu" });
    expect(menu).toHaveAttribute("aria-expanded", "false");
    await user.click(menu);
    const drawer = await screen.findByRole("dialog", { name: "Main" });
    const panel = within(drawer).getByRole("navigation", { name: "Main" });
    expect(panel).toHaveAttribute("data-variant", "drawer");
    // Focus starts on the page being shown; the drawer has no collapse toggle and no back/forward.
    await waitFor(() => expect(within(panel).getByRole("link", { name: "Library" })).toHaveFocus());
    expect(within(panel).queryByRole("button", { name: "Collapse sidebar" })).toBeNull();
    expect(within(panel).queryByRole("button", { name: "Back" })).toBeNull();

    // Choosing a page closes it.
    await user.click(within(panel).getByRole("link", { name: /^Rename/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/rename"));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Main" })).toBeNull());
  });

  it("closes the drawer with Esc and gives focus back to the menu button", async () => {
    const user = userEvent.setup();
    stubMedia(["(max-width: 1023.98px)"]);
    renderShell();
    const menu = await screen.findByRole("button", { name: "Menu" });
    await user.click(menu);
    await screen.findByRole("dialog", { name: "Main" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Main" })).toBeNull());
    expect(menu).toHaveFocus();
  });

  it("closes the drawer when a link on the page changes the address", async () => {
    stubMedia(["(max-width: 1023.98px)"]);
    const router = renderShell();
    await screen.findByRole("button", { name: "Menu" });
    useShellStore.setState({ navSheetOpen: true });
    await screen.findByRole("dialog", { name: "Main" });
    await router.navigate({ to: "/scrape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Main" })).toBeNull());
  });

  it("raises the control sizes on a touch screen", async () => {
    stubMedia(["(pointer: coarse)"]);
    renderShell();
    await screen.findByRole("heading", { name: "Page /library" });
    expect(document.documentElement.dataset.touch).toBe("true");
  });
});
