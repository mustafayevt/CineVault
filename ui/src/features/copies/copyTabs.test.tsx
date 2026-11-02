import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  useParams,
} from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { restorePlatform, stubPlatform } from "../../test/platform";
import { DeleteConfirmDialog } from "../delete/DeleteConfirmDialog";
import { useDeleteStore } from "../delete/deleteActions";
import { setHistoryBackend } from "../history/historyBackend";
import { createSyntheticHistoryBackend } from "../history/syntheticHistory";
import { setLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { createSyntheticLibraryOpsBackend } from "../library-ops/syntheticLibraryOps";
import { useSettingsStore } from "../settings/settingsStore";
import { useToastStore } from "../toasts/toastStore";
import { CopyActions, CopyTabs } from "./CopyTabs";
import { tabLabel, tabOrder, tabTexts, useCopyGroup } from "./copyGroup";
import { setCopiesBackend } from "./copiesBackend";
import type { Comparison, CopiesBackend, CopyRow } from "./copiesBackend";

const GB = 1_000_000_000;
const size = (bytes: number) => `${(bytes / GB).toFixed(1)} GB`;

function copy(id: string, over: Partial<CopyRow> = {}): CopyRow {
  return {
    id,
    title: "Example Movie",
    year: 2020,
    media: {
      resolution: "1080p",
      hdr_formats: [],
      audio: "AAC 5.1",
      video_bitrate_kbps: 6600,
      duration_minutes: 112,
    },
    source: "WEB-DL",
    size_bytes: 6.1 * GB,
    folder: `D:\\Movies\\copy-${id}`,
    file_name: `Example.Movie.2020.${id}.mkv`,
    source_label: "Media (D:)",
    best: false,
    same_file: false,
    ...over,
  };
}

const disc = copy("2", {
  media: {
    resolution: "2160p",
    hdr_formats: ["HDR10"],
    audio: "TrueHD Atmos 7.1",
    video_bitrate_kbps: 62_800,
    duration_minutes: 112,
  },
  source: "BluRay",
  size_bytes: 58.3 * GB,
  best: true,
});
const web = copy("1");

function page(over: Partial<Comparison> = {}): Comparison {
  // The Compare page puts the opened copy first; the tabs must not follow that order.
  return { copies: [web, disc], kept: false, look_same: false, same_file: false, ...over };
}

/** A copies backend: `ids` have copies, the comparison is `group`. */
function backend(group: Comparison | null, over: Partial<CopiesBackend> = {}) {
  const ids = group?.copies.map((row) => row.id) ?? [];
  const fake = {
    copiesOf: vi.fn((id: string) =>
      Promise.resolve(ids.includes(id) ? { ids, kept: group?.kept ?? false } : null),
    ),
    compare: vi.fn(() => Promise.resolve(group)),
    keepAll: vi.fn(() => Promise.resolve()),
    keepCopy: vi.fn((_keep: string, others: readonly string[]) => Promise.resolve([...others])),
    askAgain: vi.fn(() => Promise.resolve()),
    labelToDrop: vi.fn(() => Promise.resolve<string | null>(null)),
    ...over,
  };
  setCopiesBackend(fake);
  return fake;
}

/** The copy-specific part of a Movie page: the tabs, the shown copy's id and the Copies row. */
function MovieStub() {
  const { movieId = "" } = useParams({ strict: false }) as { movieId?: string };
  const group = useCopyGroup(movieId);
  return (
    <>
      <p>Movie {movieId}</p>
      {group === null ? null : (
        <>
          <CopyTabs group={group} movieId={movieId} />
          <CopyActions group={group} movieId={movieId} />
        </>
      )}
    </>
  );
}

async function renderMovie(movieId: string) {
  const root = createRootRoute({
    component: () => (
      <>
        <DeleteConfirmDialog />
        <Outlet />
      </>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        component: () => <p>Library</p>,
      }),
      createRoute({ getParentRoute: () => root, path: "/movies/$movieId", component: MovieStub }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId/copies",
        component: () => <p>Compare page</p>,
      }),
      createRoute({ getParentRoute: () => root, path: "/rename", component: () => <p>Rename</p> }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await act(async () => {
    await router.navigate({ to: "/movies/$movieId", params: { movieId } });
  });
  return router;
}

const tabs = async () => within(await screen.findByRole("navigation")).getAllByRole("link");

beforeEach(() => {
  stubPlatform("windows");
  useToastStore.setState({ toasts: [], history: [] });
  useDeleteStore.setState({ hidden: new Set(), request: null, confirm: null });
  useSettingsStore.setState({ values: {}, status: "ready" });
  setLibraryOpsBackend(createSyntheticLibraryOpsBackend());
});
afterEach(() => {
  restorePlatform();
  setCopiesBackend(null);
  setHistoryBackend(null);
  setLibraryOpsBackend(null);
});

describe("the copy tabs", () => {
  it("read the resolution and HDR, adding the source and then the size only where tabs clash", () => {
    expect(tabTexts([disc, web], size)).toEqual(["2160p HDR10", "1080p"]);
    const blu = copy("3", { source: "BluRay" });
    expect(tabTexts([web, blu], size)).toEqual(["1080p · WEB-DL", "1080p · BluRay"]);
    const twin = copy("4", { size_bytes: 7 * GB });
    expect(tabTexts([web, twin, disc], size)).toEqual([
      "1080p · WEB-DL · 6.1 GB",
      "1080p · WEB-DL · 7.0 GB",
      "2160p HDR10",
    ]);
    expect(tabTexts([copy("5", { media: null }), web], size)).toEqual(["Not read yet", "1080p"]);
    const dv = copy("6", {
      media: {
        resolution: "2160p",
        hdr_formats: ["Dolby Vision", "HDR10"],
        audio: "TrueHD Atmos 7.1",
        video_bitrate_kbps: 62_800,
        duration_minutes: 112,
      },
    });
    expect(tabTexts([dv], size)).toEqual(["2160p DV"]);
  });

  it("describe each copy in full for the tooltip: resolution, HDR, source and size", () => {
    expect(tabLabel(disc, size)).toBe("2160p · HDR10 · BluRay · 58.3 GB");
    expect(tabLabel(web, size)).toBe("1080p · WEB-DL · 6.1 GB");
    expect(tabLabel(copy("3", { media: null, source: "" }), size)).toBe("Not read yet · 6.1 GB");
  });

  it("order the best copy first, then the larger, then the lower id", () => {
    const small = copy("3", { size_bytes: 2 * GB });
    const twin = copy("4");
    expect(tabOrder([twin, small, web, disc]).map((row) => row.id)).toEqual(["2", "1", "4", "3"]);
  });

  it("show one tab per copy in the same order on both copies, the shown one current", async () => {
    backend(page());
    const router = await renderMovie("1");
    const [first, second] = (await tabs()) as [HTMLElement, HTMLElement];
    expect(first).toHaveAccessibleName(/^2160p · HDR10 · BluRay · 58\.3\sGB, best quality$/);
    expect(first).toHaveAttribute("href", "/movies/2");
    expect(first).not.toHaveAttribute("aria-current");
    expect(second).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("navigation")).toHaveAccessibleName("2 copies of this movie");

    await userEvent.setup().click(first);
    await screen.findByText("Movie 2");
    const [again] = (await tabs()) as [HTMLElement];
    expect(again).toHaveAttribute("aria-current", "page");
    expect(again).toHaveAccessibleName(/2160p/);
    // The switch replaced the history entry: Back leaves the movie.
    act(() => router.history.back());
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
  });

  it("are still there after Keep both", async () => {
    backend(page({ kept: true }));
    await renderMovie("1");
    expect(await tabs()).toHaveLength(2);
  });

  it("are absent with the Copies row for a movie without copies", async () => {
    const copies = backend(null);
    await renderMovie("9");
    await screen.findByText("Movie 9");
    await waitFor(() => expect(copies.copiesOf).toHaveBeenCalledWith("9"));
    expect(screen.queryByRole("navigation")).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete this copy" })).toBeNull();
    // The comparison is not even asked for.
    expect(copies.compare).not.toHaveBeenCalled();
  });
});

describe("the Copies row", () => {
  it("offers Keep only this one, Delete this copy and Compare", async () => {
    backend(page());
    await renderMovie("1");
    expect(await screen.findByRole("button", { name: "Keep only this one" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Delete this copy" })).toBeEnabled();
    expect(screen.getByRole("link", { name: "Compare" })).toHaveAttribute(
      "href",
      "/movies/1/copies",
    );
  });

  it("offers no Keep only this one when the copies may be one file", async () => {
    backend(page({ same_file: true }));
    await renderMovie("1");
    expect(await screen.findByRole("button", { name: "Delete this copy" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep only this one" })).toBeNull();
  });

  it("Keep only this one deletes the other copies with the usual Delete and stays", async () => {
    const user = userEvent.setup();
    const history = createSyntheticHistoryBackend();
    const deleteTitles = vi.spyOn(history, "deleteTitles");
    setHistoryBackend(history);
    const copies = backend(page());
    const router = await renderMovie("1");

    await user.click(await screen.findByRole("button", { name: "Keep only this one" }));
    const dialog = await screen.findByRole("dialog", {
      name: /^Delete “Example Movie \(2020\)”\?$/,
    });
    expect(copies.keepCopy).toHaveBeenCalledWith("1", ["2"]);
    expect(deleteTitles).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(deleteTitles).toHaveBeenCalledWith(["2"]));
    await waitFor(() => expect(useToastStore.getState().toasts[0]?.undo).toBeDefined());
    expect(router.state.location.pathname).toBe("/movies/1");
  });

  it("Delete this copy deletes the shown copy and opens the next one in tab order", async () => {
    const user = userEvent.setup();
    const history = createSyntheticHistoryBackend();
    const deleteTitles = vi.spyOn(history, "deleteTitles");
    setHistoryBackend(history);
    useSettingsStore.setState({ values: { "delete.confirm": false }, status: "ready" });
    backend(page());
    const router = await renderMovie("1");

    await user.click(await screen.findByRole("button", { name: "Delete this copy" }));

    await waitFor(() => expect(deleteTitles).toHaveBeenCalledWith(["1"]));
    await waitFor(() => expect(router.state.location.pathname).toBe("/movies/2"));
  });

  it("asks before Delete this copy and deletes nothing on Cancel", async () => {
    const user = userEvent.setup();
    const history = createSyntheticHistoryBackend();
    const deleteTitles = vi.spyOn(history, "deleteTitles");
    setHistoryBackend(history);
    backend(page());
    const router = await renderMovie("1");

    await user.click(await screen.findByRole("button", { name: "Delete this copy" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(deleteTitles).not.toHaveBeenCalled();
    expect(router.state.location.pathname).toBe("/movies/1");
  });
});
