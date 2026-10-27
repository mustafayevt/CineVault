import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { showToast } from "../toasts/toastStore";
import type { Platform } from "./platform";
import { AppShell } from "./AppShell";
import type { WindowApi } from "./windowApi";

function fakeWindow(): WindowApi {
  return {
    minimize: vi.fn().mockResolvedValue(undefined),
    toggleMaximize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(false),
    onResized: vi.fn().mockResolvedValue(() => {}),
  };
}

function renderShell(
  platform: Platform = "windows",
  path = "/library",
  windowApi: WindowApi | null = fakeWindow(),
) {
  const rootRoute = createRootRoute({
    component: () => <AppShell platform={platform} windowApi={windowApi} />,
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
      component: () => <h1>{`Page ${to}`}</h1>,
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

describe("the app frame", () => {
  it("floats the sidebar panel inside an 8px gutter, with the page flat on the window ground", async () => {
    renderShell();
    const panel = await screen.findByRole("navigation", { name: "Main" });
    // The gutter is the panel wrapper's padding on every side: 8px above, left, below and to
    // the content.
    expect(panel.parentElement?.className).toContain("p-(--gutter)");
    expect(panel.className).toContain("rounded-(--r-14)");
    expect(panel.className).toContain("bg-surface-floating");
    expect(panel.className).toContain("shadow-floating");
    expect(screen.getByTestId("app-frame").className).toContain("bg-surface-window");
    const main = screen.getByRole("main");
    expect(main.className).not.toMatch(/bg-|shadow|rounded/);
    expect(main).toContainElement(screen.getByRole("heading", { name: "Page /library" }));
  });

  it("puts the title bar only over the content, as a drag region", async () => {
    renderShell();
    const row = await screen.findByTestId("title-row");
    expect(row).toHaveAttribute("data-tauri-drag-region");
    expect(row.className).toContain("h-[38px]");
    expect(screen.getByRole("navigation", { name: "Main" })).not.toContainElement(row);
  });

  it("installs the desktop guards: F5 does nothing and Ctrl+F focuses the title-bar search", async () => {
    renderShell();
    await screen.findByTestId("title-row");
    const reload = new KeyboardEvent("keydown", { key: "F5", bubbles: true, cancelable: true });
    document.body.dispatchEvent(reload);
    expect(reload.defaultPrevented).toBe(true);
    await userEvent.setup().keyboard("{Control>}f{/Control}");
    expect(screen.getByRole("combobox", { name: "Search library" })).toHaveFocus();
  });

  it("shows the caption buttons on Windows only", async () => {
    const api = fakeWindow();
    renderShell("windows", "/library", api);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Minimize" }));
    expect(api.minimize).toHaveBeenCalled();
  });

  it("leaves the caption buttons to the system on macOS", async () => {
    renderShell("macos");
    await screen.findByRole("navigation", { name: "Main" });
    expect(screen.queryByRole("button", { name: "Minimize" })).toBeNull();
  });

  it("fades a new page in, keyed by the page and not by its search", async () => {
    const router = renderShell();
    const first = (await screen.findByRole("heading", { name: "Page /library" })).parentElement;
    expect(first?.className).toContain("cv-page");
    await act(() => router.navigate({ to: "/library", search: { q: "heat" } as never }));
    expect(screen.getByRole("heading", { name: "Page /library" }).parentElement).toBe(first);
    await act(() => router.navigate({ to: "/scrape" }));
    const next = (await screen.findByRole("heading", { name: "Page /scrape" })).parentElement;
    expect(next).not.toBe(first);
    expect(next?.className).toContain("cv-page");
  });

  it("stacks toasts at the bottom right", async () => {
    renderShell();
    await screen.findByRole("navigation", { name: "Main" });
    act(() => void showToast({ message: "Saved 2 posters" }));
    const region = screen.getByRole("region", { name: "Toasts" });
    await waitFor(() => expect(region).toHaveTextContent("Saved 2 posters"));
    expect(region.className).toContain("right-4");
  });
});
