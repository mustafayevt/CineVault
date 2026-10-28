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
import { validateLibrarySearch } from "../../router";
import { setHistoryBackend } from "../history/historyBackend";
import type { HistoryBackend } from "../history/historyBackend";
import { createSyntheticHistoryBackend } from "../history/syntheticHistory";
import { LibraryPage } from "../library/LibraryPage";
import { setLibraryBackend } from "../library/libraryBackend";
import { resetLibraryStore } from "../library/libraryStore";
import { createSyntheticBackend } from "../library/syntheticLibrary";
import { setLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { createSyntheticLibraryOpsBackend } from "../library-ops/syntheticLibraryOps";
import { useSettingsStore } from "../settings/settingsStore";
import { useToastStore } from "../toasts/toastStore";
import {
  deleteToBin,
  erasePermanently,
  requestDelete,
  requestPermanentDelete,
  useDeleteStore,
} from "./deleteActions";
import { DeleteConfirmDialog } from "./DeleteConfirmDialog";
import { isTextField } from "./deleteKeys";
import { withoutHidden } from "./hiddenTitles";
import { PermanentDeleteDialog } from "./PermanentDeleteDialog";
import type { Listing } from "../library/useLibraryListing";

function renderApp(children: React.ReactNode = null) {
  const root = createRootRoute({
    component: () => (
      <>
        <DeleteConfirmDialog />
        <PermanentDeleteDialog />
        <Outlet />
        {children}
      </>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        validateSearch: validateLibrarySearch,
        component: LibraryPage,
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
  return router;
}

/** A History backend that records what Delete and permanent delete asked for. */
function spiedHistory() {
  const backend = createSyntheticHistoryBackend();
  const deleteTitles = vi.spyOn(backend, "deleteTitles");
  const eraseTitles = vi.spyOn(backend, "eraseTitles");
  const undo = vi.spyOn(backend, "undo");
  setHistoryBackend(backend);
  return { backend, deleteTitles, eraseTitles, undo };
}

beforeEach(() => {
  stubLayout();
  stubPlatform("windows");
  window.localStorage.clear();
  resetLibraryStore();
  setLibraryBackend(createSyntheticBackend(181));
  setLibraryOpsBackend(createSyntheticLibraryOpsBackend());
  useToastStore.setState({ toasts: [], history: [] });
  useDeleteStore.setState({ hidden: new Set(), request: null, confirm: null });
  useSettingsStore.setState({ values: {}, status: "ready" });
});
afterEach(() => {
  restoreLayout();
  restorePlatform();
  setLibraryBackend(null);
  setLibraryOpsBackend(null);
  setHistoryBackend(null);
});

const tile = async (index: number) => {
  const cell = await waitFor(() => {
    const found = document.querySelector<HTMLElement>(`[data-index="${index}"][data-id]`);
    if (found === null) throw new Error(`no poster ${index}`);
    return found;
  });
  const button = cell.querySelector("button");
  if (button === null) throw new Error("no button");
  return button;
};

describe("Delete from the Library", () => {
  it("asks first with the Delete key, deletes on Enter and offers Undo", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles, undo } = spiedHistory();
    renderApp();
    await user.click(await tile(1));
    const id = (await tile(1)).closest<HTMLElement>("[data-id]")?.dataset.id ?? "";
    await user.keyboard("{Delete}");

    const dialog = await screen.findByRole("dialog", { name: /^Delete “.*”\?$/ });
    expect(
      within(dialog).getByText(
        "The folder goes to the Recycle Bin. You can undo this from History.",
      ),
    ).toBeInTheDocument();
    const list = within(dialog).getByRole("list", { name: "What will be deleted" });
    expect(await within(list).findByText(/GB/)).toBeInTheDocument();
    // Focus starts on Delete, so Enter deletes.
    expect(within(dialog).getByRole("button", { name: "Delete" })).toHaveFocus();
    expect(deleteTitles).not.toHaveBeenCalled();
    await user.keyboard("{Enter}");

    await waitFor(() => expect(deleteTitles).toHaveBeenCalledTimes(1));
    expect(deleteTitles).toHaveBeenCalledWith([id]);
    expect(eraseTitles).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();

    // The toast says where it went and carries the Undo bound to the History entry.
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    const toast = useToastStore.getState().toasts[0];
    expect(toast?.message).toMatch(/^Deleted .* to the Recycle Bin$/);
    await act(async () => {
      await useToastStore.getState().undo(toast?.id ?? "");
    });
    expect(undo).toHaveBeenCalledTimes(1);
  });

  it("hides the title from the list at once and brings it back when the backend refuses it", async () => {
    const user = userEvent.setup();
    const backend: HistoryBackend = {
      ...createSyntheticHistoryBackend(),
      deleteTitles: (ids) =>
        new Promise((resolve) => {
          // The answer comes only after the test has looked at the list.
          setTimeout(
            () =>
              resolve({
                deleted: [],
                refused: ids.map((id) => ({ id, title: "Heat", reason: "Its drive is offline." })),
                entry_id: null,
              }),
            200,
          );
        }),
    };
    setHistoryBackend(backend);
    renderApp();
    const filters = await screen.findByRole("group", { name: "Filters" });
    await waitFor(() => expect(filters.textContent).toMatch(/181 movies/));
    await user.click(await tile(1));
    await user.keyboard("{Delete}");
    await user.click(await screen.findByTestId("confirm-delete"));
    await waitFor(() => expect(filters.textContent).toMatch(/180 movies/));
    await waitFor(() => expect(filters.textContent).toMatch(/181 movies/));
  });

  it("opens the confirmation with Shift+Delete and erases nothing until DELETE is typed", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    renderApp();
    await user.click(await tile(2));
    await user.keyboard("{Shift>}{Delete}{/Shift}");

    const dialog = await screen.findByRole("dialog", { name: /Delete 1 title permanently\?/ });
    expect(
      await within(dialog).findByText(/This skips the Recycle Bin and cannot be undone/),
    ).toBeInTheDocument();
    // What will be erased: the title, its folder and size.
    const list = within(dialog).getByRole("list", { name: "What will be erased" });
    expect(await within(list).findByText(/D:\\Movies\\Title/)).toBeInTheDocument();
    expect(within(list).getByText(/GB/)).toBeInTheDocument();

    const confirm = within(dialog).getByRole("button", { name: "Delete permanently" });
    expect(confirm).toBeDisabled();
    await user.type(within(dialog).getByRole("textbox"), "delet");
    expect(confirm).toBeDisabled();
    await user.clear(within(dialog).getByRole("textbox"));
    await user.type(within(dialog).getByRole("textbox"), "DELETE");
    expect(confirm).toBeEnabled();
    expect(eraseTitles).not.toHaveBeenCalled();

    await user.click(confirm);
    await waitFor(() => expect(eraseTitles).toHaveBeenCalledTimes(1));
    expect(eraseTitles.mock.calls[0]?.[1]).toBe("DELETE");
    expect(deleteTitles).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() =>
      expect(useToastStore.getState().toasts.at(-1)?.message).toBe("Permanently deleted 1 title"),
    );
    // A permanent delete cannot be undone.
    expect(useToastStore.getState().toasts.at(-1)?.undo).toBeUndefined();
  });

  it("sends the titles to the Recycle Bin instead from the dialog", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    renderApp();
    await user.click(await tile(0));
    await user.keyboard("{Shift>}{Delete}{/Shift}");
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Send to Recycle Bin instead" }));
    await waitFor(() => expect(deleteTitles).toHaveBeenCalledTimes(1));
    expect(eraseTitles).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("names the Trash on Linux, in the dialog and in the toast", async () => {
    stubPlatform("linux");
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    renderApp();
    await user.click(await tile(0));
    await user.keyboard("{Shift>}{Delete}{/Shift}");
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/This skips the Trash and cannot be undone/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Send to Trash instead" }));
    await waitFor(() => expect(deleteTitles).toHaveBeenCalledTimes(1));
    expect(eraseTitles).not.toHaveBeenCalled();
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(useToastStore.getState().toasts[0]?.message).toMatch(/^Deleted .* to the Trash$/);
  });

  it("closes the dialog with Cancel and Esc without touching anything", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    renderApp();
    await user.click(await tile(0));
    await user.keyboard("{Shift>}{Delete}{/Shift}");
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    (await tile(0)).closest<HTMLElement>("[role='group']")?.focus();
    await user.keyboard("{Shift>}{Delete}{/Shift}");
    await screen.findByRole("dialog");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(deleteTitles).not.toHaveBeenCalled();
    expect(eraseTitles).not.toHaveBeenCalled();
  });

  it("names the titles it would refuse apart and does not send them", async () => {
    const user = userEvent.setup();
    const { eraseTitles } = spiedHistory();
    const ops = createSyntheticLibraryOpsBackend();
    setLibraryOpsBackend({
      ...ops,
      planRemoval: (ids) =>
        Promise.resolve({
          rows: [
            {
              id: ids[0] ?? "1",
              kind: "movie",
              title: "Cats",
              year: 2019,
              folder: String.raw`E:\Archive\Movies\Cats (2019)`,
              folder_use: "own",
              files: 7,
              bytes: 7_900_000_000,
              children: 0,
              on_disk: true,
            },
          ],
          skipped: [{ id: "9", title: "The Room", reason: "Its drive is offline." }],
        }),
    });
    renderApp();
    act(() =>
      requestPermanentDelete([
        { id: "5", title: "Cats (2019)" },
        { id: "9", title: "The Room (2003)" },
      ]),
    );
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Cats (2019)")).toBeInTheDocument();
    expect(within(dialog).getByText("The Room")).toBeInTheDocument();
    expect(within(dialog).getByText("Its drive is offline.")).toBeInTheDocument();
    await user.type(within(dialog).getByRole("textbox"), "DELETE");
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(eraseTitles).toHaveBeenCalledTimes(1));
    expect(eraseTitles.mock.calls[0]?.[0]).toEqual(["5"]);
  });
});

describe("the Delete confirmation", () => {
  it("cancels with Esc and with Cancel, deleting nothing", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    renderApp();
    await user.click(await tile(3));
    await user.keyboard("{Delete}");
    await screen.findByRole("dialog");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    act(() => requestDelete([{ id: "4", title: "Heat (1995)" }]));
    const dialog = await screen.findByRole("dialog", { name: "Delete “Heat (1995)”?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(deleteTitles).not.toHaveBeenCalled();
    expect(eraseTitles).not.toHaveBeenCalled();
  });

  it("names several titles by count, lists at most five with the total and says where they go", async () => {
    const user = userEvent.setup();
    const { deleteTitles } = spiedHistory();
    useSettingsStore.setState({ values: { "files.deleted-mode": "folder" }, status: "ready" });
    renderApp();
    const targets = ["1", "2", "3", "4", "5", "6", "7"].map((id) => ({ id, title: `Title ${id}` }));
    act(() => requestDelete(targets));
    const dialog = await screen.findByRole("dialog", { name: "Delete 7 titles?" });
    expect(
      within(dialog).getByText(
        "The folders go to the trash folder on their drives. You can undo this from History.",
      ),
    ).toBeInTheDocument();
    const list = within(dialog).getByRole("list", { name: "What will be deleted" });
    expect(await within(list).findByText("and 2 more titles")).toBeInTheDocument();
    expect(within(list).getAllByRole("listitem")).toHaveLength(6);
    expect(within(dialog).getByTestId("delete-total").textContent).toMatch(/GB/);
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(deleteTitles).toHaveBeenCalledWith(targets.map((target) => target.id)),
    );
  });

  it("stops asking after Don't ask again, and the setting brings the question back", async () => {
    const user = userEvent.setup();
    const { deleteTitles } = spiedHistory();
    renderApp();
    act(() => requestDelete([{ id: "4", title: "Heat (1995)" }]));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("checkbox", { name: "Don't ask again" }));
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(deleteTitles).toHaveBeenCalledTimes(1));
    expect(useSettingsStore.getState().values["delete.confirm"]).toBe(false);

    act(() => requestDelete([{ id: "5", title: "Alien (1979)" }]));
    await waitFor(() => expect(deleteTitles).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("dialog")).toBeNull();

    act(() => useSettingsStore.getState().set("delete.confirm", true));
    act(() => requestDelete([{ id: "6", title: "Amadeus (1984)" }]));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(deleteTitles).toHaveBeenCalledTimes(2);
  });

  it("under Delete permanently always asks for the typed DELETE, even after Don't ask again", async () => {
    const user = userEvent.setup();
    const { deleteTitles, eraseTitles } = spiedHistory();
    useSettingsStore.setState({
      values: { "files.deleted-mode": "erase", "delete.confirm": false },
      status: "ready",
    });
    renderApp();
    await user.click(await tile(2));
    await user.keyboard("{Delete}");
    const dialog = await screen.findByRole("dialog", { name: /Delete 1 title permanently\?/ });
    expect(
      await within(dialog).findByText(/This skips the Recycle Bin and cannot be undone/),
    ).toBeInTheDocument();
    expect(within(dialog).queryByRole("checkbox")).toBeNull();
    // There is no Recycle Bin to fall back to under this choice.
    expect(
      within(dialog).queryByRole("button", { name: "Send to Recycle Bin instead" }),
    ).toBeNull();
    const confirm = within(dialog).getByRole("button", { name: "Delete permanently" });
    expect(confirm).toBeDisabled();
    await user.type(within(dialog).getByRole("textbox"), "DELETE");
    await user.click(confirm);
    await waitFor(() => expect(eraseTitles).toHaveBeenCalledTimes(1));
    expect(eraseTitles.mock.calls[0]?.[1]).toBe("DELETE");
    expect(deleteTitles).not.toHaveBeenCalled();
  });
});

describe("the actions behind the keys", () => {
  it("never acts for a key typed in a text field", () => {
    const input = document.createElement("input");
    const area = document.createElement("textarea");
    const button = document.createElement("button");
    expect(isTextField(input)).toBe(true);
    expect(isTextField(area)).toBe(true);
    expect(isTextField(button)).toBe(false);
    expect(isTextField(null)).toBe(false);
  });

  it("Delete on nothing does nothing, and a failed Delete shows the titles again", async () => {
    const backend = createSyntheticHistoryBackend();
    vi.spyOn(backend, "deleteTitles").mockRejectedValue(new Error("The disk is busy."));
    setHistoryBackend(backend);
    expect(await deleteToBin([])).toBe(false);
    expect(await deleteToBin([{ id: "4", title: "Heat" }])).toBe(false);
    expect(useDeleteStore.getState().hidden.size).toBe(0);
  });

  it("erasePermanently passes exactly what the dialog typed to the backend", async () => {
    const { eraseTitles } = spiedHistory();
    await erasePermanently([{ id: "4", title: "Heat" }], "DELETE");
    expect(eraseTitles).toHaveBeenCalledWith(["4"], "DELETE");
  });

  it("lists without the hidden titles and numbers the rest", () => {
    const rows = ["a", "b", "c", "d"];
    const base = {
      total: 4,
      loading: false,
      error: null,
      rowAt: (index: number) => rows[index] as never,
      keys: rows,
      setRange: vi.fn(),
      version: "1",
      seen: new Map(),
      bytesOf: () => undefined,
      totalBytes: null,
    } satisfies Listing;
    expect(withoutHidden(base, new Set())).toBe(base);
    const view = withoutHidden(base, new Set(["b"]));
    expect(view.total).toBe(3);
    expect(view.keys).toEqual(["a", "c", "d"]);
    expect(view.rowAt(1)).toBe("c");
    expect(view.rowAt(3)).toBeUndefined();
    view.setRange({ start: 1, end: 2 });
    expect(base.setRange).toHaveBeenCalledWith({ start: 2, end: 3 });
  });
});
