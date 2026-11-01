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
import { restorePlatform, stubPlatform } from "../../test/platform";
import { DeleteConfirmDialog } from "../delete/DeleteConfirmDialog";
import { PermanentDeleteDialog } from "../delete/PermanentDeleteDialog";
import { useDeleteStore } from "../delete/deleteActions";
import { setHistoryBackend } from "../history/historyBackend";
import { createSyntheticHistoryBackend } from "../history/syntheticHistory";
import { setLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { createSyntheticLibraryOpsBackend } from "../library-ops/syntheticLibraryOps";
import { useSettingsStore } from "../settings/settingsStore";
import { useNotificationStore } from "../notifications/notificationStore";
import { handleEscape } from "../shell/escStack";
import { useToastStore } from "../toasts/toastStore";
import { ComparePage } from "./ComparePage";
import { verdicts } from "./compareModel";
import { setCopiesBackend } from "./copiesBackend";
import type { Comparison, CopiesBackend, CopyRow } from "./copiesBackend";

const GB = 1_000_000_000;

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

const disc = copy("1", {
  media: {
    resolution: "2160p",
    hdr_formats: ["Dolby Vision", "HDR10"],
    audio: "TrueHD Atmos 7.1",
    video_bitrate_kbps: 62_800,
    duration_minutes: 112,
  },
  source: "BluRay",
  size_bytes: 58.3 * GB,
  best: true,
});
const web = copy("2");
const third = copy("3", {
  media: {
    resolution: "720p",
    hdr_formats: [],
    audio: "AAC 2.0",
    video_bitrate_kbps: 3400,
    duration_minutes: 112,
  },
  source: "WEBRip",
  size_bytes: 2.9 * GB,
});

/** A copies backend over one page; the calls are spies. */
function backend(page: Comparison | null, over: Partial<CopiesBackend> = {}) {
  const fake = {
    copiesOf: vi.fn(() => Promise.resolve(null)),
    compare: vi.fn(() => Promise.resolve(page)),
    keepAll: vi.fn(() => Promise.resolve()),
    keepCopy: vi.fn((_keep: string, others: readonly string[]) => Promise.resolve([...others])),
    askAgain: vi.fn(() => Promise.resolve()),
    labelToDrop: vi.fn(() => Promise.resolve<string | null>(null)),
    ...over,
  };
  setCopiesBackend(fake);
  return fake;
}

function spiedHistory() {
  const history = createSyntheticHistoryBackend();
  const deleteTitles = vi.spyOn(history, "deleteTitles");
  const eraseTitles = vi.spyOn(history, "eraseTitles");
  setHistoryBackend(history);
  return { deleteTitles, eraseTitles };
}

async function renderPage(movieId = "1") {
  const root = createRootRoute({
    component: () => (
      <>
        <DeleteConfirmDialog />
        <PermanentDeleteDialog />
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
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId",
        component: () => <p>Movie page</p>,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/movies/$movieId/copies",
        component: ComparePage,
      }),
      createRoute({ getParentRoute: () => root, path: "/rename", component: () => <p>Rename</p> }),
      createRoute({
        getParentRoute: () => root,
        path: "/settings",
        component: () => <p>Settings</p>,
      }),
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
    await router.navigate({ to: "/movies/$movieId/copies", params: { movieId } });
  });
  return router;
}

beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
  useDeleteStore.setState({ hidden: new Set(), request: null, confirm: null });
  useSettingsStore.setState({ values: {}, status: "ready" });
  setLibraryOpsBackend(createSyntheticLibraryOpsBackend());
});
afterEach(() => {
  restoreLayout();
  restorePlatform();
  setCopiesBackend(null);
  setHistoryBackend(null);
  setLibraryOpsBackend(null);
});

const cards = () => screen.findAllByRole("article");

describe("the Compare copies page", () => {
  it("shows one card per copy, the opened copy first, with the values and the labels", async () => {
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    await renderPage();

    const [first, second] = await cards();
    expect(first).toHaveAccessibleName(/^Copy 1 of 2: 2160p · Dolby Vision on Media \(D:\)$/);
    expect(second).toHaveAccessibleName(/^Copy 2 of 2: 1080p on Media \(D:\)$/);
    expect(within(first as HTMLElement).getByText("2160p · 4K")).toBeInTheDocument();
    expect(within(first as HTMLElement).getByText("Dolby Vision · HDR10")).toBeInTheDocument();
    expect(within(first as HTMLElement).getByText("TrueHD Atmos 7.1")).toBeInTheDocument();
    expect(within(first as HTMLElement).getByText("BluRay")).toBeInTheDocument();
    expect(within(first as HTMLElement).getByText("62.8 Mb/s")).toBeInTheDocument();
    expect(within(first as HTMLElement).getByText("1h 52m")).toBeInTheDocument();
    expect(within(second as HTMLElement).getByText("1080p · Full HD")).toBeInTheDocument();
    expect(within(second as HTMLElement).getByText("SDR")).toBeInTheDocument();
    expect(within(second as HTMLElement).getByText("WEB-DL")).toBeInTheDocument();
    // Plain hints beside the rows.
    expect(screen.getByText("How sharp the picture is")).toBeInTheDocument();
    expect(screen.getByText("Higher keeps more detail")).toBeInTheDocument();
    // The smaller copy says by how much.
    expect(within(second as HTMLElement).getByText(/smaller$/)).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 1, name: /Compare copies · Example Movie/ }),
    ).toBeInTheDocument();
  });

  it("marks the better value of each row and Best quality on one card only", async () => {
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    await renderPage();
    const [first, second] = (await cards()) as [HTMLElement, HTMLElement];

    expect(screen.getAllByText("Best quality")).toHaveLength(1);
    expect(within(first).getByText("Best quality")).toBeInTheDocument();
    const verdict = (card: HTMLElement, row: string) =>
      card.querySelector(`[data-row="${row}"]`)?.getAttribute("data-verdict");
    for (const row of ["resolution", "hdr", "audio", "source", "bitrate"]) {
      expect(verdict(first, row), row).toBe("win");
      expect(verdict(second, row), row).toBe("lose");
    }
    // Length is the same on both; Size is only compared by the note.
    expect(verdict(first, "length")).toBe("same");
    expect(verdict(first, "size")).toBe("plain");
    expect(within(first).getAllByRole("img", { name: "Better" })).toHaveLength(5);
  });

  it("offers Keep this one on every card, marked or not, and Keep both once", async () => {
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    await renderPage();
    const [first, second] = (await cards()) as [HTMLElement, HTMLElement];

    expect(within(first).getByRole("button", { name: /^Keep this one/ })).toBeEnabled();
    expect(within(second).getByRole("button", { name: /^Keep this one/ })).toBeEnabled();
    expect(screen.getAllByRole("button", { name: "Keep both" })).toHaveLength(1);
    expect(screen.getByText("Keeps every file. These stop showing under Copies.")).toBeVisible();
  });

  it("explains where the other copies go and links Settings › Deleted files", async () => {
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    await renderPage();
    await cards();

    const link = screen.getByRole("link", { name: "Settings › Deleted files" });
    expect(link).toHaveAttribute("href", expect.stringContaining("section=deleted-files"));
    expect(
      screen.getByText("Right now that is the Recycle Bin, so Undo brings the other copy back."),
    ).toBeInTheDocument();
  });

  it("names the destination of the setting, with the count of the other copies", async () => {
    useSettingsStore.setState({
      values: { "files.deleted-mode": "folder" },
      status: "ready",
    });
    backend({ copies: [disc, web, third], kept: false, look_same: false, same_file: false });
    await renderPage();
    await cards();

    expect(
      screen.getByText(
        "Right now that is a trash folder on each drive, so Undo brings the other 2 copies back.",
      ),
    ).toBeInTheDocument();
  });

  it("lays out three copies and says Keep all", async () => {
    backend({ copies: [disc, web, third], kept: false, look_same: false, same_file: false });
    await renderPage();

    expect(await cards()).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Keep all" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep both" })).toBeNull();
    expect(screen.getAllByRole("button", { name: /^Keep this one/ })).toHaveLength(3);
  });

  it("offers no Keep this one when the copies may be one file reached two ways", async () => {
    const local = copy("1", { same_file: true });
    const share = copy("2", { same_file: true, folder: "\\\\SERVER\\D-Cinema\\copy-1" });
    backend({ copies: [local, share], kept: false, look_same: false, same_file: true });
    await renderPage();
    await cards();

    expect(screen.queryByRole("button", { name: /^Keep this one/ })).toBeNull();
    expect(
      screen.getByText(/^These may be the same file reached through two library folders\./),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Settings › Library folders" })).toHaveAttribute(
      "href",
      expect.stringContaining("section=folders"),
    );
    // Nothing on the page talks about deleting; keeping both still records a decision only.
    expect(screen.queryByRole("link", { name: "Settings › Deleted files" })).toBeNull();
    expect(screen.getByRole("button", { name: "Keep both" })).toBeEnabled();
  });

  it("says when the copies look the same, and what keeping one frees", async () => {
    const twin = copy("2", { size_bytes: 6.1 * GB });
    backend({ copies: [copy("1"), twin], kept: false, look_same: true, same_file: false });
    await renderPage();
    await cards();

    expect(screen.getByText("These two copies look the same.")).toBeInTheDocument();
    expect(screen.getByText(/^Keeping one frees 6\.1 GB/)).toBeInTheDocument();
    expect(screen.queryByText("Best quality")).toBeNull();
  });

  it("warns when the lengths differ by more than five minutes and says nothing about freeing space", async () => {
    const cut = copy("2", {
      media: {
        resolution: "1080p",
        hdr_formats: [],
        audio: "AAC 5.1",
        video_bitrate_kbps: 6600,
        duration_minutes: 98,
      },
    });
    backend({ copies: [copy("1"), cut], kept: false, look_same: false, same_file: false });
    await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    expect(
      screen.getByText("Lengths differ by 14 min — these may be different films."),
    ).toBeVisible();
    expect(screen.queryByText(/^Keeping one frees/)).toBeNull();
    expect(first.querySelector('[data-row="length"]')).toHaveClass("text-warn");
    expect(within(first).getByRole("img", { name: "Lengths differ" })).toBeInTheDocument();
  });

  it("does not warn about lengths a few minutes apart", async () => {
    const credits = copy("2", {
      media: {
        resolution: "1080p",
        hdr_formats: [],
        audio: "AAC 5.1",
        video_bitrate_kbps: 6600,
        duration_minutes: 115,
      },
    });
    backend({ copies: [copy("1"), credits], kept: false, look_same: false, same_file: false });
    await renderPage();
    await cards();

    expect(screen.queryByText(/^Lengths differ/)).toBeNull();
  });

  it("says Not read yet for a copy whose media info is missing and marks no winner", async () => {
    const unread = copy("2", { media: null, source: "" });
    backend({ copies: [copy("1"), unread], kept: false, look_same: false, same_file: false });
    await renderPage();
    const [, second] = (await cards()) as [HTMLElement, HTMLElement];

    expect(within(second).getAllByText("Not read yet").length).toBeGreaterThan(0);
    expect(screen.queryByRole("img", { name: "Better" })).toBeNull();
    expect(within(second).getByRole("button", { name: /^Keep this one/ })).toBeEnabled();
  });

  it("says there are no other copies when the movie has none left", async () => {
    backend(null);
    await renderPage();

    expect(
      await screen.findByRole("heading", { name: "This movie has no other copies" }),
    ).toBeVisible();
    expect(screen.getByRole("link", { name: "Back to the movie" })).toHaveAttribute(
      "href",
      "/movies/1",
    );
  });

  it("returns to where it was opened from with Esc and with Cancel", async () => {
    const user = userEvent.setup();
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    const router = await renderPage();
    await cards();

    // The shell turns Esc into the innermost layer's close; the page registered as a picker.
    act(() => {
      expect(handleEscape({ defaultPrevented: false, target: document.body })).toBe(true);
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));

    await act(async () => {
      await router.navigate({ to: "/movies/$movieId/copies", params: { movieId: "1" } });
    });
    await user.click(await screen.findByRole("button", { name: /^Cancel/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
  });
});

describe("Keep this one", () => {
  it("deletes only the copies the backend confirmed as they are now", async () => {
    const user = userEvent.setup();
    const { deleteTitles } = spiedHistory();
    useSettingsStore.setState({ values: { "delete.confirm": false }, status: "ready" });
    // The page is stale: copy 3 left the group meanwhile, and the backend leaves it out.
    const copies = backend(
      { copies: [disc, web, third], kept: false, look_same: false, same_file: false },
      { keepCopy: vi.fn(() => Promise.resolve(["2"])) },
    );
    await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));

    expect(copies.keepCopy).toHaveBeenCalledWith("1", ["2", "3"]);
    await waitFor(() => expect(deleteTitles).toHaveBeenCalledWith(["2"]));
  });

  it("deletes nothing and says why when the backend refuses", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    useSettingsStore.setState({ values: { "delete.confirm": false }, status: "ready" });
    useNotificationStore.getState().clear();
    const why =
      "The copy to keep is no longer in the library or cannot be reached, so nothing was deleted.";
    backend(
      { copies: [disc, web], kept: false, look_same: false, same_file: false },
      { keepCopy: vi.fn(() => Promise.reject(new Error(why))) },
    );
    await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));

    await waitFor(() =>
      expect(useNotificationStore.getState().items[0]).toMatchObject({
        severity: "error",
        title: "Nothing was deleted",
        detail: why,
      }),
    );
    expect(deleteTitles).not.toHaveBeenCalled();
    expect(eraseTitles).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("lets no other card start a second Keep this one while the first waits", async () => {
    const user = userEvent.setup();
    spiedHistory();
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    await renderPage();
    const [first, second] = (await cards()) as [HTMLElement, HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));
    await screen.findByRole("dialog", { name: /^Delete “Example Movie \(2020\)”\?$/ });

    // The open confirmation hides the page from assistive technology; the buttons wait.
    const keepOf = (card: HTMLElement) =>
      within(card).getByRole("button", { name: /^Keep this one/, hidden: true });
    expect(keepOf(first)).toBeDisabled();
    expect(keepOf(second)).toBeDisabled();
  });

  it("asks first, then deletes only the other copy with the usual Delete and offers Undo", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    const copies = backend({
      copies: [disc, web],
      kept: false,
      look_same: false,
      same_file: false,
    });
    const router = await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));
    const dialog = await screen.findByRole("dialog", {
      name: /^Delete “Example Movie \(2020\)”\?$/,
    });
    expect(deleteTitles).not.toHaveBeenCalled();
    expect(copies.labelToDrop).toHaveBeenCalledWith("1");
    // Each copy to delete shows its folder, so two copies of one title can be told apart.
    await waitFor(() =>
      expect(within(dialog).getByRole("list")).toHaveTextContent(String.raw`D:\Movies\Title 2`),
    );
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(deleteTitles).toHaveBeenCalledWith(["2"]));
    expect(eraseTitles).not.toHaveBeenCalled();
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toMatch(/to the Recycle Bin$/);
    expect(toast?.undo).toBeDefined();
    // The opened copy was kept: back to where the page was opened from.
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
  });

  it("works on the copy that is not marked and opens its page when the opened copy goes", async () => {
    const user = userEvent.setup();
    const { deleteTitles } = spiedHistory();
    useSettingsStore.setState({ values: { "delete.confirm": false }, status: "ready" });
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    const router = await renderPage();
    const [, second] = (await cards()) as [HTMLElement, HTMLElement];

    await user.click(within(second).getByRole("button", { name: /^Keep this one/ }));

    // Ask before deleting is off: the 2160p copy goes at once.
    await waitFor(() => expect(deleteTitles).toHaveBeenCalledWith(["1"]));
    await waitFor(() => expect(router.state.location.pathname).toBe("/movies/2"));
  });

  it("opens the permanent-delete confirmation under Delete permanently and erases nothing before Yes", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    useSettingsStore.setState({ values: { "files.deleted-mode": "erase" }, status: "ready" });
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    const router = await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));
    const dialog = await screen.findByRole("dialog", { name: /permanently\?/ });
    expect(deleteTitles).not.toHaveBeenCalled();
    expect(eraseTitles).not.toHaveBeenCalled();

    // Cancelling deletes nothing and leaves the page as it was.
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(deleteTitles).not.toHaveBeenCalled();
    expect(eraseTitles).not.toHaveBeenCalled();
    expect(router.state.location.pathname).toBe("/movies/1/copies");
  });

  it("offers to drop a copy label on the toast and opens the Rename preview for the kept movie", async () => {
    const user = userEvent.setup();
    spiedHistory();
    useSettingsStore.setState({ values: { "delete.confirm": false }, status: "ready" });
    const copies = backend(
      { copies: [disc, web], kept: false, look_same: false, same_file: false },
      { labelToDrop: vi.fn(() => Promise.resolve<string | null>("copy 2")) },
    );
    const router = await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));

    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    const action = useToastStore.getState().toasts[0]?.action;
    expect(action?.label).toBe("Rename to drop the '- copy 2' label?");
    expect(copies.labelToDrop).toHaveBeenCalledWith("1");
    // Nothing is renamed by itself: the offer only opens the preview of that movie.
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    act(() => action?.onClick());
    await waitFor(() => expect(router.state.location.pathname).toBe("/rename"));
    expect(router.state.location.search).toMatchObject({ ids: "1" });
  });

  it("offers no label to drop for a plain name", async () => {
    const user = userEvent.setup();
    spiedHistory();
    useSettingsStore.setState({ values: { "delete.confirm": false }, status: "ready" });
    backend({ copies: [disc, web], kept: false, look_same: false, same_file: false });
    await renderPage();
    const [first] = (await cards()) as [HTMLElement];

    await user.click(within(first).getByRole("button", { name: /^Keep this one/ }));

    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(useToastStore.getState().toasts[0]?.action).toBeUndefined();
  });
});

describe("Keep both and Ask again", () => {
  it("records the decision with an Undo toast and leaves the page", async () => {
    const user = userEvent.setup();
    const copies = backend({
      copies: [disc, web],
      kept: false,
      look_same: false,
      same_file: false,
    });
    const router = await renderPage();
    await cards();

    await user.click(screen.getByRole("button", { name: "Keep both" }));

    await waitFor(() => expect(copies.keepAll).toHaveBeenCalledWith(["1", "2"]));
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toBe("Kept both copies of Example Movie (2020)");
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    await act(async () => {
      await useToastStore.getState().undo(toast?.id ?? "");
    });
    expect(copies.askAgain).toHaveBeenCalledWith("1");
  });

  it("says a group was kept and asks again on request", async () => {
    const user = userEvent.setup();
    const copies = backend({ copies: [disc, web], kept: true, look_same: false, same_file: false });
    await renderPage();
    await cards();

    expect(screen.getByText("You chose to keep these copies")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep both" })).toBeNull();
    // Keep this one stays available on a kept group.
    expect(screen.getAllByRole("button", { name: /^Keep this one/ })).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "Ask again" }));
    await waitFor(() => expect(copies.askAgain).toHaveBeenCalledWith("1"));
  });
});

describe("the better value of a row", () => {
  it("names the highest as the winner, equal ranks as the same and unread copies as no winner", () => {
    expect(verdicts([5, 3])).toEqual(["win", "lose"]);
    expect(verdicts([3, 3])).toEqual(["same", "same"]);
    expect(verdicts([5, 5, 2])).toEqual(["win", "win", "lose"]);
    expect(verdicts([5, null])).toEqual(["plain", "plain"]);
    expect(verdicts([5])).toEqual(["plain"]);
  });
});
