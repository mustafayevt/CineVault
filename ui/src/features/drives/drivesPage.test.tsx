import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
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
import { restorePlatform, stubPlatform } from "../../test/platform";
import { DrivesPage } from "./DrivesPage";
import { setDrivesBackend } from "./drivesBackend";
import type { DrivesBackend } from "./drivesBackend";
import { agoText, qualityShares, usedShare } from "./drivesModel";
import type { DrivesOverview } from "./drivesModel";
import { validateDrivesSearch } from "./drivesSearch";
import { syntheticDrives } from "./syntheticDrives";
import { useToastStore } from "../toasts/toastStore";

function renderDrives(at = "/drives") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/drives",
        validateSearch: validateDrivesSearch,
        component: DrivesPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/settings",
        validateSearch: (search: Record<string, unknown>) => ({
          section: typeof search.section === "string" ? search.section : undefined,
        }),
        component: () => <p>Settings page</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [at] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return { router, client };
}

function backendOf(overview: () => DrivesOverview): DrivesBackend & { fire: () => void } {
  const handlers = new Set<() => void>();
  return {
    overview: () => Promise.resolve(overview()),
    onChanged: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    fire: () => handlers.forEach((handler) => handler()),
    trashContents: () => Promise.resolve({ entries: [], bytes: 0 }),
    emptyTrash: () => Promise.resolve({ bytes: 0, problems: [] }),
  };
}

beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  window.localStorage.clear();
});
afterEach(() => {
  restoreLayout();
  restorePlatform();
  setDrivesBackend(null);
  vi.useRealTimers();
});

describe("the Drives page", () => {
  it("says how many drives there are and how many are offline", async () => {
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives();
    expect(await screen.findByText("4 drives · 1 offline")).toBeVisible();
    expect(screen.getByText("Updated just now")).toBeVisible();
  });

  it("shows a card per drive with its space, folders and quality mix", async () => {
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives();
    const media = await screen.findByRole("button", { name: "Show the titles on D: Media" });
    expect(within(media).getByText("1.21 TB free of 7.28 TB")).toBeVisible();
    expect(within(media).getByText(String.raw`D:\Movies`)).toBeVisible();
    expect(within(media).getByText("142 titles")).toBeVisible();
    expect(within(media).getByText("not in library")).toBeVisible();
    expect(within(media).getByText("2160p 38")).toBeVisible();
    const archive = screen.getByRole("button", { name: "Show the titles on E: Archive" });
    expect(within(archive).getByText("18 shows")).toBeVisible();
    expect(within(archive).getByText("39 titles")).toBeVisible();
  });

  it("marks a low drive with the warning and an offline drive with when it was last seen", async () => {
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives();
    const system = await screen.findByRole("button", { name: "Show the titles on C: System" });
    expect(within(system).getByText("Low: 41 GB free")).toBeVisible();
    expect(within(system).getByRole("meter")).toHaveAttribute("data-state", "low");
    const nas = screen.getByRole("button", { name: "Show the titles on Z: NAS" });
    expect(within(nas).getByText("Offline")).toBeVisible();
    expect(within(nas).getByText("last seen 2 days ago")).toBeVisible();
    expect(nas).toHaveAttribute("data-state", "offline");
    // The titles stay listed on the offline card.
    expect(within(nas).getByText(String.raw`Z:\Shows`)).toBeVisible();
  });

  it("selects the drive a card names and shows its heading", async () => {
    const user = userEvent.setup();
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives();
    const media = await screen.findByRole("button", { name: "Show the titles on D: Media" });
    expect(media).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(String.raw`142 titles in D:\Movies`)).toBeVisible();
    const archive = screen.getByRole("button", { name: "Show the titles on E: Archive" });
    await user.click(archive);
    expect(archive).toHaveAttribute("aria-pressed", "true");
    expect(media).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByText("57 titles in 2 folders")).toBeVisible();
    // The keyboard chooses a card too.
    const system = screen.getByRole("button", { name: "Show the titles on C: System" });
    system.focus();
    await user.keyboard("{Enter}");
    expect(system).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps the chosen drive in the address, so Back returns to the one before", async () => {
    const user = userEvent.setup();
    setDrivesBackend(backendOf(() => syntheticDrives()));
    const { router } = renderDrives();
    const archive = await screen.findByRole("button", { name: "Show the titles on E: Archive" });
    await user.click(archive);
    await waitFor(() => expect(router.state.location.search).toEqual({ drive: "E:" }));
    router.history.back();
    const media = screen.getByRole("button", { name: "Show the titles on D: Media" });
    await waitFor(() => expect(media).toHaveAttribute("aria-pressed", "true"));
  });

  it("opens on the drive the address names", async () => {
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives("/drives?drive=E%3A");
    const archive = await screen.findByRole("button", { name: "Show the titles on E: Archive" });
    expect(archive).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("57 titles in 2 folders")).toBeVisible();
  });

  it("falls back to the first drive for an unknown drive in the address", async () => {
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives("/drives?drive=Q%3A");
    const media = await screen.findByRole("button", { name: "Show the titles on D: Media" });
    expect(media).toHaveAttribute("aria-pressed", "true");
  });

  it("reads every drive again on Refresh and keeps the numbers when that fails", async () => {
    const user = userEvent.setup();
    useToastStore.setState({ toasts: [], history: [] });
    let reads = 0;
    let fail = false;
    let release: () => void = () => undefined;
    const backend = backendOf(() => syntheticDrives());
    setDrivesBackend({
      ...backend,
      overview: () => {
        reads += 1;
        if (fail) return Promise.reject(new Error("the disk did not answer"));
        if (reads === 1) return backend.overview();
        return new Promise((resolve) => {
          release = () => resolve(syntheticDrives());
        });
      },
    });
    renderDrives();
    const refresh = await screen.findByRole("button", { name: "Refresh" });
    expect(reads).toBe(1);
    await user.click(refresh);
    // While it reads it says so and does not start a second read.
    const busy = await screen.findByRole("button", { name: "Refreshing…" });
    expect(busy).toHaveAttribute("aria-busy", "true");
    await user.click(busy);
    expect(reads).toBe(2);
    act(() => release());
    expect(await screen.findByRole("button", { name: "Refresh" })).toBeEnabled();
    expect(screen.getByText("Updated just now")).toBeVisible();

    fail = true;
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(useToastStore.getState().toasts.map((toast) => toast.message)).toContain(
        "Couldn’t read the drives. Showing the last numbers.",
      ),
    );
    expect(screen.getByRole("button", { name: "Show the titles on D: Media" })).toBeVisible();
  });

  it("reads the numbers again when the service says they changed", async () => {
    let free = 1.21e12;
    const backend = backendOf(() => {
      const overview = syntheticDrives();
      const [first, ...rest] = overview.drives;
      return first === undefined
        ? overview
        : { ...overview, drives: [{ ...first, freeBytes: free }, ...rest] };
    });
    setDrivesBackend(backend);
    renderDrives();
    const media = await screen.findByRole("button", { name: "Show the titles on D: Media" });
    expect(within(media).getByText("1.21 TB free of 7.28 TB")).toBeVisible();
    free = 1.4e12;
    act(() => backend.fire());
    await waitFor(() => expect(within(media).getByText("1.4 TB free of 7.28 TB")).toBeVisible());
  });

  it("offers Settings when no drive holds a library folder", async () => {
    setDrivesBackend(backendOf(() => ({ drives: [], updatedMs: Date.now() })));
    renderDrives();
    expect(await screen.findByText("No drives to show yet")).toBeVisible();
    expect(screen.getByRole("button", { name: "Open Settings" })).toBeEnabled();
  });
});

describe("the drive numbers", () => {
  it("measures the used share and the quality mix", () => {
    expect(usedShare({ totalBytes: 1000, freeBytes: 250 })).toBeCloseTo(0.75);
    expect(usedShare({ totalBytes: 0, freeBytes: 0 })).toBe(0);
    const shares = qualityShares({ p2160: 38, p1080: 89, p720: 12, sd: 0 });
    expect(shares.map((share) => share.id)).toEqual(["2160p", "1080p", "720p"]);
    expect(shares.reduce((sum, share) => sum + share.percent, 0)).toBeCloseTo(100);
  });

  it("names the bin the Trash on Linux, on a drive card", async () => {
    stubPlatform("linux");
    setDrivesBackend(backendOf(() => syntheticDrives()));
    renderDrives();
    const media = await screen.findByRole("button", { name: "Show the titles on D: Media" });
    expect(within(media).getByText("In the Trash from CineVault")).toBeVisible();
  });

  it("writes how long ago a drive was seen", () => {
    const now = Date.UTC(2026, 9, 27, 12);
    expect(agoText(now - 30_000, now, "en")).toBe("");
    expect(agoText(now - 5 * 60_000, now, "en")).toBe("5 minutes ago");
    expect(agoText(now - 2 * 86_400_000, now, "en")).toBe("2 days ago");
  });

  it("shows the trash of a drive and empties it after a Yes or No", async () => {
    const user = userEvent.setup();
    const emptied: (string | null)[] = [];
    let overview = syntheticDrives();
    const backend = {
      ...backendOf(() => overview),
      trashContents: () =>
        Promise.resolve({
          entries: [
            {
              kind: "folder" as const,
              path: String.raw`D:\Movies\.deletedByCineVault1`,
              bytes: 2.4e9,
            },
            { kind: "bin" as const, path: String.raw`D:\Movies\Heat (1995)`, bytes: 3.6e9 },
          ],
          bytes: 6e9,
        }),
      emptyTrash: (driveId: string | null) => {
        emptied.push(driveId);
        overview = {
          ...overview,
          drives: overview.drives.map((d) => ({ ...d, trashBytes: 0, binBytes: 0 })),
        };
        return Promise.resolve({ bytes: 6e9, problems: [] });
      },
    };
    setDrivesBackend(backend);
    renderDrives();
    const media = await screen.findByRole("button", { name: "Show the titles on D: Media" });
    expect(within(media).getByText("Trash folder")).toBeVisible();
    expect(within(media).getByText("2.4 GB")).toBeVisible();
    expect(within(media).getByText("In the Recycle Bin from CineVault")).toBeVisible();
    // A drive without trash has no Empty button.
    const archive = screen.getByRole("button", { name: "Show the titles on E: Archive" });
    expect(within(archive).queryByText("Empty…")).toBeNull();

    await user.click(within(media).getByRole("button", { name: "Empty the trash of D: Media" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(String.raw`D:\Movies\Heat (1995)`)).toBeVisible();
    const confirm = within(dialog).getByRole("button", { name: "Empty for good" });
    expect(within(dialog).queryByRole("textbox")).toBeNull();
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    await waitFor(() => expect(emptied).toEqual(["D:"]));
    await waitFor(() => expect(within(media).queryByText("Trash folder")).toBeNull());
  });
});
