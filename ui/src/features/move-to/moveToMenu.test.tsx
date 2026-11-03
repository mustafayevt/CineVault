import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Outlet,
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
import { DrivesPage } from "../drives/DrivesPage";
import { setDrivesBackend } from "../drives/drivesBackend";
import { syntheticDrives } from "../drives/syntheticDrives";
import { setLibraryBackend } from "../library/libraryBackend";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { setLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import type { LibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { createSyntheticLibraryOpsBackend } from "../library-ops/syntheticLibraryOps";
import { setRecoveryBackend } from "../activity/recoveryBackend";
import type { RecoveryBackend, UnfinishedBatch } from "../activity/recoveryBackend";
import { useRecoveryStore } from "../activity/resolveUnfinished";
import { useToastStore } from "../toasts/toastStore";
import { MoveToHost } from "./MoveToMenu";
import { useMoveToStore } from "./moveToStore";

const LONG_PATH = String.raw`E:\Archive\Backups of the old server\Films in 4K remux quality\Movies`;

function renderDrives(ops: LibraryOpsBackend = createSyntheticLibraryOpsBackend()) {
  setLibraryOpsBackend(ops);
  const root = createRootRoute({
    component: () => (
      <>
        <MoveToHost />
        <Outlet />
      </>
    ),
  });
  const page = createRoute({ getParentRoute: () => root, path: "/drives", component: DrivesPage });
  const settings = createRoute({
    getParentRoute: () => root,
    path: "/settings",
    component: () => <h1>Settings</h1>,
  });
  const router = createRouter({
    routeTree: root.addChildren([page, settings]),
    history: createMemoryHistory({ initialEntries: ["/drives"] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

/** Selects three titles on the Drives page; returns the bar and its Move to… button. */
async function selectTitles() {
  const user = userEvent.setup();
  const table = await screen.findByRole("grid", { name: "Titles on D: Media" });
  await waitFor(() => expect(within(table).getAllByRole("checkbox").length).toBeGreaterThan(4));
  const boxes = within(table).getAllByRole("checkbox");
  for (const box of boxes.slice(1, 4)) await user.click(box);
  const bar = await screen.findByRole("toolbar", { name: "Bulk actions" });
  const button = within(bar).getByRole("button", { name: "Move to…" });
  return { user, table, bar, button };
}

async function loadedMenu() {
  const menu = await screen.findByRole("menu", { name: "Move to" });
  await waitFor(() => expect(within(menu).queryAllByRole("menuitem").length).toBeGreaterThan(0));
  return menu;
}

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
  useMoveToStore.setState({ request: null, running: null });
  const drives = syntheticDrives();
  setDrivesBackend({
    overview: () => Promise.resolve(drives),
    onChanged: () => () => undefined,
    trashContents: () => Promise.resolve({ entries: [], bytes: 0 }),
    emptyTrash: () => Promise.resolve({ bytes: 0, problems: [] }),
  });
  setLibraryBackend(createSyntheticBackend(30));
});
afterEach(() => {
  restoreLayout();
  setDrivesBackend(null);
  setLibraryBackend(null);
  setLibraryOpsBackend(null);
  setRecoveryBackend(null);
});

describe("opening and closing the Move to menu", () => {
  it("opens every time, and Esc closes it but keeps the selection", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    for (let round = 0; round < 3; round += 1) {
      await user.click(button);
      expect(await loadedMenu()).toBeVisible();
      await user.keyboard("{Escape}");
      await waitFor(() => expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull());
      expect(screen.getByRole("toolbar", { name: "Bulk actions" })).toBeVisible();
    }
    // Focus goes back to the button that opened it.
    expect(button).toHaveFocus();
  });

  it("closes when Move to… is pressed again, and opens on the next press", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    await user.click(button);
    await loadedMenu();
    await user.click(button);
    await waitFor(() => expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull());
    await user.click(button);
    expect(await loadedMenu()).toBeVisible();
  });

  it("closes on a click outside, and that click still reaches the list", async () => {
    renderDrives();
    const { user, table, button } = await selectTitles();
    await user.click(button);
    await loadedMenu();
    const [fifth] = within(table).getAllByRole("checkbox").slice(5);
    if (fifth === undefined) throw new Error("no fifth row");
    expect(fifth).not.toBeChecked();
    await user.click(fifth);
    await waitFor(() => expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull());
    expect(fifth).toBeChecked();
    expect(within(screen.getByRole("toolbar")).getByText("4 selected")).toBeVisible();
  });

  it("opens again after the selection bar re-renders with a new selection", async () => {
    renderDrives();
    const { user, table, button } = await selectTitles();
    const [sixth] = within(table).getAllByRole("checkbox").slice(6);
    if (sixth === undefined) throw new Error("no sixth row");
    await user.click(sixth);
    expect(within(screen.getByRole("toolbar")).getByText("4 selected")).toBeVisible();
    await user.click(button);
    const menu = await loadedMenu();
    expect(within(menu).getByText(/^Move 4 titles · .* to$/)).toBeVisible();
  });

  it("closes when the selection is cleared", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    await user.click(button);
    await loadedMenu();
    act(() => useMoveToStore.getState().request?.anchor.element?.remove());
    await waitFor(() => expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull());
  });
});

describe("choosing a destination", () => {
  it("starts the move for a destination that can take the titles", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await loadedMenu();
    await user.click(within(menu).getByRole("menuitem", { name: /E: Archive/ }));
    await waitFor(() =>
      expect(
        useToastStore.getState().toasts.some((toast) => /^Moved 3 titles/.test(toast.message)),
      ).toBe(true),
    );
    expect(screen.queryByRole("menu", { name: "Move to" })).toBeNull();
  });

  it("keeps a disabled destination from being chosen and says why", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await loadedMenu();
    const videos = within(menu).getByRole("menuitem", { name: /Videos/ });
    await user.click(videos);
    expect(screen.getByRole("menu", { name: "Move to" })).toBeVisible();
    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(videos).toHaveAccessibleDescription(/^Not enough space: needs .*, 41\sGB free\.$/);
  });

  it("works from the keyboard: arrows, Enter on a disabled row does nothing, Enter moves", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    button.focus();
    // Past the press that selected the titles, so the request counts as a keyboard one.
    const now = Date.now();
    const realNow = Date.now;
    Date.now = () => now + 5000;
    try {
      await user.keyboard("{Enter}");
      const menu = await loadedMenu();
      const archive = within(menu).getByRole("menuitem", { name: /E: Archive/ });
      await waitFor(() => expect(archive).toHaveFocus());
      await user.keyboard("{ArrowDown}");
      const next = document.activeElement;
      expect(next).toHaveAttribute("aria-disabled", "true");
      await user.keyboard("{Enter}");
      expect(screen.getByRole("menu", { name: "Move to" })).toBeVisible();
      expect(useToastStore.getState().toasts).toHaveLength(0);
      await user.keyboard("{Home}");
      expect(archive).toHaveFocus();
      await user.keyboard("{Enter}");
    } finally {
      Date.now = realNow;
    }
    await waitFor(() =>
      expect(
        useToastStore.getState().toasts.some((toast) => /^Moved 3 titles/.test(toast.message)),
      ).toBe(true),
    );
  });
});

describe("what the menu shows", () => {
  it("shortens a long path in the middle and keeps the whole path as its tooltip", async () => {
    const ops = createSyntheticLibraryOpsBackend();
    const listSources = ops.listSources;
    renderDrives({
      ...ops,
      listSources: async () =>
        (await listSources()).map((source) =>
          source.id === 2 ? { ...source, path: LONG_PATH } : source,
        ),
    });
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await loadedMenu();
    const row = within(menu)
      .getAllByRole("menuitem")
      .find((item) => item.textContent?.includes("Films in 4K"));
    if (row === undefined) throw new Error("no row for the long path");
    expect(within(row).getByTitle(LONG_PATH)).toBeVisible();
    // The last folder stays whole; the folders above it give way.
    expect(within(row).getByText("Movies", { exact: true })).toBeVisible();
  });

  it("says when there is no other folder to move to and offers Settings", async () => {
    const ops = createSyntheticLibraryOpsBackend();
    const listSources = ops.listSources;
    const router = renderDrives({
      ...ops,
      listSources: async () =>
        (await listSources()).filter(
          (source) => source.id !== 2 && source.id !== 5 && source.id !== 6,
        ),
    });
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await screen.findByRole("menu", { name: "Move to" });
    expect(
      await within(menu).findByText("There is no other movie folder to move to."),
    ).toBeVisible();
    expect(within(menu).getByText("Add one in Settings › Library folders.")).toBeVisible();
    await user.click(within(menu).getByRole("menuitem", { name: "Open Settings" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/settings"));
    expect(router.state.location.search).toEqual({ section: "folders" });
  });

  it("says while the free space is measured", async () => {
    const ops = createSyntheticLibraryOpsBackend();
    renderDrives({ ...ops, planMove: () => new Promise(() => undefined) });
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await screen.findByRole("menu", { name: "Move to" });
    expect(within(menu).getByRole("status")).toHaveTextContent("Measuring free space…");
  });

  it("disables every destination while another move runs", async () => {
    renderDrives();
    const { user, button } = await selectTitles();
    act(() =>
      useMoveToStore
        .getState()
        .setRunning({ label: "Heat", target: "E:", percent: 10, paused: false }),
    );
    await user.click(button);
    const menu = await loadedMenu();
    const archive = within(menu).getByRole("menuitem", { name: /E: Archive/ });
    expect(archive).toHaveAttribute("aria-disabled", "true");
    expect(archive).toHaveAccessibleDescription("Another move is running. Wait for it to finish.");
  });
});

describe("a title held by a move that did not finish", () => {
  const held: UnfinishedBatch = {
    batch_id: "move-9",
    kind: "move",
    created_ms: Date.now(),
    titles: ["Title 1"],
    items: 1,
    files_done: 8,
    files_total: 10,
    reason: "CineVault closed before the move finished.",
    // Every sample title, so whichever are selected one of them is held.
    item_ids: Array.from({ length: 30 }, (_, n) => String(n + 1)),
    destination: String.raw`F:\Movie`,
    bytes_done: 15e9,
    bytes_total: 86e9,
    auto_resume: false,
  };

  function recovery(): RecoveryBackend & { batches: UnfinishedBatch[] } {
    const backend = {
      batches: [held],
      list: vi.fn(() => Promise.resolve(backend.batches)),
      resume: vi.fn(() =>
        Promise.resolve({ batch_id: "move-9", completed: 1, stopped: null, notes: [] }),
      ),
      rollBack: vi.fn(() => {
        backend.batches = [];
        return Promise.resolve({ batch_id: "move-9", restored: 1, removed: 9, problems: [] });
      }),
    };
    return backend;
  }

  beforeEach(() => useRecoveryStore.setState({ active: null, working: {} }));

  it("says how far it got and rolls it back from the menu, which stays open", async () => {
    const backend = recovery();
    setRecoveryBackend(backend);
    renderDrives();
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await loadedMenu();
    expect(
      await within(menu).findByText(
        String.raw`A move of Title 1 to F:\Movie did not finish (8 of 10 files copied, 15 GB of 86 GB).`,
      ),
    ).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Resume that move" })).toBeVisible();
    expect(within(menu).getByRole("menuitem", { name: "Open in Activity" })).toBeVisible();

    await user.click(within(menu).getByRole("menuitem", { name: "Roll it back" }));
    expect(backend.rollBack).toHaveBeenCalledWith("move-9");
    await waitFor(() =>
      expect(within(menu).queryByRole("menuitem", { name: "Roll it back" })).toBeNull(),
    );
    expect(screen.getByRole("menu", { name: "Move to" })).toBeVisible();
  });

  it("resumes it from the menu", async () => {
    const backend = recovery();
    setRecoveryBackend(backend);
    renderDrives();
    const { user, button } = await selectTitles();
    await user.click(button);
    const menu = await loadedMenu();
    await user.click(await within(menu).findByRole("menuitem", { name: "Resume that move" }));
    expect(backend.resume).toHaveBeenCalledWith("move-9", expect.any(Function));
  });
});
