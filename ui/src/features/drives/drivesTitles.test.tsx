import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { setLibraryBackend } from "../library/libraryBackend";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { setLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { createSyntheticLibraryOpsBackend } from "../library-ops/syntheticLibraryOps";
import { MoveToHost } from "../move-to/MoveToMenu";
import { useMoveToStore } from "../move-to/moveToStore";
import { useToastStore } from "../toasts/toastStore";
import { DrivesPage } from "./DrivesPage";
import { setDrivesBackend } from "./drivesBackend";
import { sortDriveTitles } from "./titlesOfDrive";
import type { DriveTitle } from "./titlesOfDrive";
import { syntheticDrives } from "./syntheticDrives";

function renderDrives() {
  const root = createRootRoute({
    component: () => (
      <>
        <MoveToHost />
        <Outlet />
      </>
    ),
  });
  const page = createRoute({ getParentRoute: () => root, path: "/drives", component: DrivesPage });
  const router = createRouter({
    routeTree: root.addChildren([page]),
    history: createMemoryHistory({ initialEntries: ["/drives"] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
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
  setLibraryOpsBackend(createSyntheticLibraryOpsBackend());
});
afterEach(() => {
  restoreLayout();
  setDrivesBackend(null);
  setLibraryBackend(null);
  setLibraryOpsBackend(null);
});

const title = (name: string, bytes: number, year = 2000): DriveTitle => ({
  id: name,
  kind: "movie",
  title: name,
  year,
  quality: null,
  hdr: null,
  height: 0,
  bytes,
  path: `D:\\Movies\\${name}`,
  folder: name,
  source: "Movies",
  filesMissing: false,
});

describe("sorting the titles of a drive", () => {
  it("puts the biggest first and breaks ties by title", () => {
    const sorted = sortDriveTitles([title("B", 10), title("A", 10), title("C", 30)], {
      column: "size",
      direction: "desc",
    });
    expect(sorted.map((entry) => entry.title)).toEqual(["C", "A", "B"]);
  });

  it("sorts by year and by title in either direction", () => {
    const list = [title("B", 1, 2010), title("A", 2, 1990), title("C", 3, 2001)];
    expect(
      sortDriveTitles(list, { column: "year", direction: "desc" }).map((e) => e.title),
    ).toEqual(["B", "C", "A"]);
    expect(
      sortDriveTitles(list, { column: "title", direction: "asc" }).map((e) => e.title),
    ).toEqual(["A", "B", "C"]);
  });
});

describe("the titles of a drive", () => {
  it("lists the drive's titles with the biggest first and a header that says where", async () => {
    renderDrives();
    const table = await screen.findByRole("grid", { name: "Titles on D: Media" });
    await waitFor(() => expect(within(table).getAllByRole("row").length).toBeGreaterThan(5));
    expect(screen.getByRole("columnheader", { name: "Size" })).toHaveAttribute(
      "aria-sort",
      "descending",
    );
    expect(screen.getByRole("button", { name: "Biggest first" })).toBeVisible();
    const sizes = within(table)
      .getAllByRole("row")
      .slice(1, 6)
      .map((row) => Number(row.querySelector("[data-bytes]")?.getAttribute("data-bytes")));
    expect([...sizes].sort((a, b) => b - a)).toEqual(sizes);
  });

  it("shows the selection bar with the count and size, Move to and Delete", async () => {
    const user = userEvent.setup();
    renderDrives();
    const table = await screen.findByRole("grid", { name: "Titles on D: Media" });
    await waitFor(() => expect(within(table).getAllByRole("checkbox").length).toBeGreaterThan(3));
    const boxes = within(table).getAllByRole("checkbox");
    for (const box of boxes.slice(1, 4)) await user.click(box);
    const bar = await screen.findByRole("toolbar", { name: "Bulk actions" });
    expect(within(bar).getByText("3 selected")).toBeVisible();
    expect(within(bar).getByRole("button", { name: "Move to…" })).toBeEnabled();
    expect(within(bar).getByRole("button", { name: /^Delete/ })).toBeVisible();
  });
});

describe("the Move to menu", () => {
  async function openMenu() {
    const user = userEvent.setup();
    renderDrives();
    const table = await screen.findByRole("grid", { name: "Titles on D: Media" });
    await waitFor(() => expect(within(table).getAllByRole("checkbox").length).toBeGreaterThan(3));
    const boxes = within(table).getAllByRole("checkbox");
    for (const box of boxes.slice(1, 4)) await user.click(box);
    const bar = await screen.findByRole("toolbar", { name: "Bulk actions" });
    await user.click(within(bar).getByRole("button", { name: "Move to…" }));
    const menu = await screen.findByRole("menu", { name: "Move to" });
    await waitFor(() => expect(within(menu).queryByText("Reading the folders…")).toBeNull());
    return { user, menu };
  }

  it("lists the other movie folders with the space before and after", async () => {
    const { menu } = await openMenu();
    expect(within(menu).getByText(/^Move 3 titles · .* to$/)).toBeVisible();
    const archive = within(menu).getByRole("menuitem", { name: /E: Archive/ });
    expect(within(archive).getByText("5.81 TB free")).toBeVisible();
    expect(within(archive).getByText(/^5\.6\d? TB$/)).toBeVisible();
    expect(within(archive).getByText(/free after/)).toBeVisible();
    // The folder the titles are in, and the TV folders, are not destinations.
    expect(within(menu).queryByRole("menuitem", { name: /D: Media/ })).toBeNull();
    expect(within(menu).queryByRole("menuitem", { name: /Shows/ })).toBeNull();
    // What the source drive frees.
    expect(within(menu).getByText(/D: Media goes from 1\.21 TB to/)).toBeVisible();
  });

  it("disables a destination without room, and an offline one, with the reason", async () => {
    const { menu } = await openMenu();
    const videos = within(menu).getByRole("menuitem", { name: /Videos/ });
    expect(videos).toHaveAttribute("aria-disabled", "true");
    const room = within(videos).getByText(/^Not enough space: needs .* GB, 41 GB free\.$/);
    expect(room).toBeVisible();
    // The reason is in the row itself, not a floating tip, and describes it for screen readers.
    expect(videos).toHaveAccessibleDescription(room.textContent ?? "");
    expect(within(menu).queryByRole("tooltip")).toBeNull();
    const nas = within(menu).getByRole("menuitem", { name: /Z: NAS/ });
    expect(nas).toHaveAttribute("aria-disabled", "true");
    expect(within(nas).getByText("Offline")).toBeVisible();
    expect(nas).toHaveAccessibleDescription("The drive is offline. Connect it to move here.");
  });

  it("starts the move on choosing a destination and offers Undo", async () => {
    const { user, menu } = await openMenu();
    await user.click(within(menu).getByRole("menuitem", { name: /E: Archive/ }));
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => /^Moved 3 titles to E: Archive$/.test(toast.message)),
      ).toBe(true),
    );
    const toast = useToastStore.getState().toasts.find((entry) => entry.undo !== undefined);
    expect(toast?.undo).toBeDefined();
    expect(useMoveToStore.getState().running).toBeNull();
  });

  it("does not move anything when a disabled destination is chosen", async () => {
    const { user, menu } = await openMenu();
    await user.click(within(menu).getByRole("menuitem", { name: /Videos/ }));
    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(screen.getByRole("menu", { name: "Move to" })).toBeVisible();
  });
});
