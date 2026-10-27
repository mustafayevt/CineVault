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

import { MAXIMIZE_BUTTON_ID } from "./CaptionButtons";
import { installDesktopGuards } from "./desktopGuards";
import type { Platform } from "./platform";
import { TitleBar } from "./TitleBar";
import type { WindowApi } from "./windowApi";

function fakeWindow(maximized = false): WindowApi {
  return {
    minimize: vi.fn().mockResolvedValue(undefined),
    toggleMaximize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn().mockResolvedValue(maximized),
    onResized: vi.fn().mockResolvedValue(() => {}),
  };
}

function renderBar(platform: Platform, path: string, api: WindowApi | null = fakeWindow()) {
  const rootRoute = createRootRoute({
    component: () => (
      <>
        <TitleBar platform={platform} windowApi={api} />
      </>
    ),
  });
  const routes = ["/library", "/history"].map((to) =>
    createRoute({ getParentRoute: () => rootRoute, path: to, component: () => null }),
  );
  const router = createRouter({
    routeTree: rootRoute.addChildren(routes),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(<RouterProvider router={router} />);
  return router;
}

describe("the title bar", () => {
  it("is a 38px drag region with the search centred and 440px wide", async () => {
    renderBar("windows", "/library");
    const bar = await screen.findByTestId("title-row");
    expect(bar).toHaveAttribute("data-tauri-drag-region");
    expect(bar.className).toContain("h-[38px]");
    expect(bar.className).toContain("bg-transparent");
    const search = screen.getByRole("searchbox", { name: "Search library" });
    expect(search).toHaveAttribute("placeholder", "Search movies, shows, people");
    const wrapper = search.closest("form")?.firstElementChild;
    expect(wrapper?.className).toContain("w-[440px]");
    // Both sides are equal flex columns, so the field sits in the middle of the bar.
    const sides = Array.from(bar.children).filter((child) => child.tagName === "DIV");
    expect(sides).toHaveLength(2);
    for (const side of sides) {
      expect(side.className).toContain("flex-1");
      expect(side).toHaveAttribute("data-tauri-drag-region");
    }
  });

  it("shows the Ctrl F hint on Windows", async () => {
    renderBar("windows", "/library");
    expect(await screen.findByText("Ctrl F")).toBeInTheDocument();
  });

  it("shows the command key hint on macOS and no caption buttons", async () => {
    renderBar("macos", "/library");
    expect(await screen.findByText("⌘ F")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Minimize" })).toBeNull();
  });

  it("puts the Windows caption buttons flush at the top right and wires them", async () => {
    const api = fakeWindow();
    renderBar("windows", "/library", api);
    const group = await screen.findByRole("group", { name: "Window controls" });
    expect(group.className).toContain("h-8");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Minimize" }));
    await user.click(screen.getByRole("button", { name: "Maximize" }));
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(api.minimize).toHaveBeenCalled();
    expect(api.toggleMaximize).toHaveBeenCalled();
    expect(api.close).toHaveBeenCalled();
    expect(group.parentElement).toHaveAttribute("data-tauri-drag-region");
  });

  it("gives the maximize button the id the native snap-layout region targets", async () => {
    renderBar("windows", "/library");
    expect(MAXIMIZE_BUTTON_ID).toBe("cv-caption-maximize");
    expect(await screen.findByRole("button", { name: "Maximize" })).toHaveAttribute(
      "id",
      MAXIMIZE_BUTTON_ID,
    );
  });

  it("switches the maximize button to Restore while the window is maximized", async () => {
    renderBar("windows", "/library", fakeWindow(true));
    expect(await screen.findByRole("button", { name: "Restore" })).toHaveAttribute(
      "id",
      MAXIMIZE_BUTTON_ID,
    );
  });

  it("opens the Library with ?q= on Enter", async () => {
    const router = renderBar("windows", "/history");
    const user = userEvent.setup();
    await user.type(await screen.findByRole("searchbox"), "blade runner{Enter}");
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(router.state.location.search).toEqual({ q: "blade runner" });
  });

  it("keeps the Library's other choices when searching from the Library", async () => {
    const router = renderBar("windows", "/library?view=table");
    const user = userEvent.setup();
    await user.type(await screen.findByRole("searchbox"), "heat{Enter}");
    await waitFor(() => expect(router.state.location.search).toMatchObject({ q: "heat" }));
    expect(router.state.location.search).toMatchObject({ view: "table" });
  });

  it("shows the query of the address and clears it with Esc", async () => {
    const router = renderBar("windows", "/library?q=heat");
    const field = await screen.findByRole("searchbox");
    expect(field).toHaveValue("heat");
    await userEvent.setup().type(field, "{Escape}");
    expect(field).toHaveValue("");
    await waitFor(() => expect(router.state.location.search).toEqual({}));
  });

  it("follows the address when it changes", async () => {
    const router = renderBar("windows", "/library");
    const field = await screen.findByRole("searchbox");
    await act(() => router.navigate({ to: "/library", search: { q: "alien" } as never }));
    await waitFor(() => expect(field).toHaveValue("alien"));
  });

  it("takes focus with its text selected on Ctrl+F", async () => {
    const remove = installDesktopGuards();
    renderBar("windows", "/library?q=heat");
    const field = await screen.findByRole<HTMLInputElement>("searchbox");
    await userEvent.setup().keyboard("{Control>}f{/Control}");
    expect(field).toHaveFocus();
    expect(field.selectionStart).toBe(0);
    expect(field.selectionEnd).toBe(4);
    remove();
  });
});
