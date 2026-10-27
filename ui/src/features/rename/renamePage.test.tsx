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
import { setOpenFolderBackend } from "../detail/openFolderBackend";
import { useNotificationStore } from "../notifications/notificationStore";
import { setScanBackend, unavailableScanBackend } from "../scan/scanBackend";
import { useToastStore } from "../toasts/toastStore";
import { RenamePage } from "./RenamePage";
import { validateRenameSearch } from "./renameSearch";
import { setOrganizeBackend } from "./organizeBackend";
import type { OrganizeBackend } from "./organizeBackend";
import { createSyntheticOrganizeBackend } from "./syntheticOrganize";

function renderRename(path = "/rename") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/rename",
        validateSearch: validateRenameSearch,
        component: RenamePage,
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
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

let backend: OrganizeBackend;

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
  backend = createSyntheticOrganizeBackend();
  setOrganizeBackend(backend);
});
afterEach(() => {
  restoreLayout();
  setOrganizeBackend(null);
  setScanBackend(null);
  setOpenFolderBackend(null);
});

const list = () => screen.findByRole("list", { name: "Titles to rename" });
const rowOf = async (title: string, index = 0) => {
  const found = within(await list())
    .getAllByRole("listitem")
    .filter((item) => within(item).queryAllByText(title).length > 0);
  const row = found[index];
  if (row === undefined) throw new Error(`no row for ${title}`);
  return row;
};
const footer = () => screen.findByText(/^\d+ of \d+ selected$/);
const applyButton = () => screen.getByRole("button", { name: /^Rename \d+ titles?$/ });

describe("the Rename page", () => {
  it("has the tabs Movies and TV episodes with counts, and shows the templates with a link to Settings", async () => {
    renderRename();
    const tabs = await screen.findByRole("group", { name: "Kind" });
    await waitFor(() =>
      expect(within(tabs).getByRole("button", { name: /^Movies\s*\d+/ })).toBeVisible(),
    );
    expect(
      await within(tabs).findByRole("button", { name: /^TV episodes\s*2/ }),
    ).toBeInTheDocument();
    expect(await screen.findByText("{Title} ({Year})")).toBeInTheDocument();
    const edit = screen.getByRole("link", { name: "Edit in Settings" });
    expect(edit).toHaveAttribute("href", "/settings?section=naming");
    // The page shows the templates and never edits them.
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("lists every title that would change with a ticked checkbox, and a footer that counts them", async () => {
    renderRename();
    await list();
    const dune = await rowOf("Dune: Part Two");
    expect(within(dune).getByRole("checkbox")).toBeChecked();
    expect(dune).toHaveTextContent("2024 · F:\\Personal\\Movie");
    expect(await footer()).toHaveTextContent("4 of 7 selected");
    expect(screen.getByText("3 conflicts")).toBeInTheDocument();
    expect(applyButton()).toHaveTextContent("Rename 4 titles");
    // A title whose folder and file already match is not listed.
    expect(within(await list()).queryByText("Arrival")).toBeNull();
  });

  it("shows removed words struck through and added words highlighted, never cutting a word", async () => {
    renderRename();
    const dune = await rowOf("Dune: Part Two");
    const struck = [...dune.querySelectorAll("del")].map((node) => node.textContent ?? "");
    const added = [...dune.querySelectorAll("ins")].map((node) => node.textContent ?? "");
    expect(struck.length).toBeGreaterThan(0);
    expect(added.length).toBeGreaterThan(0);
    expect(added.some((text) => text.includes("(2024)"))).toBe(true);
    // Words are kept, struck or added whole: no segment starts or ends inside a word.
    for (const node of dune.querySelectorAll("del, ins")) {
      const previous = node.previousSibling?.textContent ?? "";
      const next = node.nextSibling?.textContent ?? "";
      const text = node.textContent ?? "";
      const cutAtStart = /[\p{L}\p{N}]$/u.test(previous) && /^[\p{L}\p{N}]/u.test(text);
      const cutAtEnd = /[\p{L}\p{N}]$/u.test(text) && /^[\p{L}\p{N}]/u.test(next);
      expect(cutAtStart || cutAtEnd).toBe(false);
    }
  });

  it("lists all the files of a title in place with Show all N files, and folds them with Hide files", async () => {
    const user = userEvent.setup();
    renderRename();
    const dune = await rowOf("Dune: Part Two");
    await user.click(within(dune).getByRole("button", { name: "Show all 3 files" }));
    const files = within(dune).getByRole("list", { name: "Files of Dune: Part Two" });
    expect(within(files).getAllByRole("listitem")).toHaveLength(3);
    const hide = within(dune).getByRole("button", { name: "Hide files" });
    expect(hide).toHaveAttribute("aria-expanded", "true");
    await user.click(hide);
    expect(within(dune).getByRole("button", { name: "Show all 3 files" })).toBeInTheDocument();
  });

  it("leaves a title out when it is unticked and renames only the others", async () => {
    const user = userEvent.setup();
    const applySpy = vi.spyOn(backend, "apply");
    renderRename();
    const dune = await rowOf("Dune: Part Two");
    await user.click(within(dune).getByRole("checkbox"));
    expect(await screen.findByText("3 of 7 selected")).toBeInTheDocument();
    expect(applyButton()).toHaveTextContent("Rename 3 titles");
    await user.click(applyButton());
    await waitFor(() => expect(applySpy).toHaveBeenCalledTimes(1));
    const ids = applySpy.mock.calls[0]?.[1] ?? [];
    expect([...ids].sort()).toEqual(["2", "3", "5"]);
  });

  it("does not count a title with a conflict until it is resolved", async () => {
    const user = userEvent.setup();
    renderRename();
    const crash = await rowOf("Crash");
    expect(within(crash).getByRole("checkbox")).toBeDisabled();
    expect(within(crash).getByRole("checkbox")).not.toBeChecked();
    expect(
      within(crash).getByText("A folder named Crash (2004) already exists in D:\\Media\\Movies."),
    ).toBeInTheDocument();
    expect(await footer()).toHaveTextContent("4 of 7 selected");
    expect(applyButton()).toHaveTextContent("Rename 4 titles");

    // Choosing the next free name resolves it, and the title joins the count.
    await user.click(within(crash).getByRole("button", { name: "Use Crash (2004) (2)" }));
    await waitFor(() => expect(screen.getByText("5 of 7 selected")).toBeInTheDocument());
    expect(applyButton()).toHaveTextContent("Rename 5 titles");
    expect(screen.getByText("2 conflicts")).toBeInTheDocument();
    expect(within(await rowOf("Crash")).getByRole("checkbox")).toBeChecked();
  });

  it("offers Open both folders and Skip on a conflict, and Skip keeps the title out", async () => {
    const user = userEvent.setup();
    const open = vi.fn(() => Promise.resolve());
    setOpenFolderBackend({ open });
    renderRename();
    const crash = await rowOf("Crash");
    await user.click(within(crash).getByRole("button", { name: "Open both folders" }));
    expect(open).toHaveBeenCalledWith("D:\\Media\\Movies\\crash.2004.1080p.bluray");
    expect(open).toHaveBeenCalledWith("D:\\Media\\Movies\\Crash (2004)");

    await user.click(within(crash).getByRole("button", { name: "Skip" }));
    const skipped = await rowOf("Crash");
    expect(await within(skipped).findByText("Skipped")).toBeInTheDocument();
    expect(applyButton()).toHaveTextContent("Rename 4 titles");
  });

  it("holds a title back for its video details with Read now and no checkbox", async () => {
    const user = userEvent.setup();
    const readPending = vi.fn(() => Promise.resolve("ready" as const));
    setScanBackend({ ...unavailableScanBackend, readPendingMediaInfo: readPending });
    renderRename();
    const held = await rowOf("Mad Max: Fury Road");
    expect(within(held).queryByRole("checkbox")).toBeNull();
    expect(within(held).getByTitle(/video details/i)).toBeInTheDocument();
    await user.click(within(held).getByRole("button", { name: "Read now" }));
    await waitFor(() => expect(readPending).toHaveBeenCalledTimes(1));
  });

  it("switches to the TV episodes tab and lists the shows there", async () => {
    const user = userEvent.setup();
    renderRename();
    await list();
    await user.click(screen.getByRole("button", { name: /^TV episodes/ }));
    const severance = await rowOf("Severance");
    expect(within(severance).getByRole("button", { name: "Show all 9 files" })).toBeInTheDocument();
    expect(await screen.findByText("2 of 2 selected")).toBeInTheDocument();
    expect(applyButton()).toHaveTextContent("Rename 2 titles");
  });

  it("renames the ticked titles, says so with an Undo toast, and undoes them", async () => {
    const user = userEvent.setup();
    const undoSpy = vi.spyOn(backend, "undo");
    renderRename();
    await user.click(await screen.findByRole("button", { name: "Rename 4 titles" }));
    await waitFor(() =>
      expect(
        useToastStore.getState().toasts.some((toast) => toast.message === "Renamed 4 titles"),
      ).toBe(true),
    );
    const toast = useToastStore
      .getState()
      .toasts.find((entry) => entry.message === "Renamed 4 titles");
    expect(toast?.undo).toBeDefined();
    // The renamed titles leave the list: nothing is left to rename but the conflicts.
    await waitFor(() => expect(screen.queryByText("Dune: Part Two")).toBeNull());
    await act(async () => {
      await toast?.undo?.();
    });
    expect(undoSpy).toHaveBeenCalledTimes(1);
    expect(
      useToastStore
        .getState()
        .toasts.some((entry) => /^Undone: 4 titles are named as before$/.test(entry.message)),
    ).toBe(true);
    expect(await screen.findByText("Dune: Part Two")).toBeInTheDocument();
  });

  it("says so when the selected titles are already named as the templates say", async () => {
    renderRename("/rename?ids=4");
    expect(
      await screen.findByText("The selected titles are already named as your templates say."),
    ).toBeInTheDocument();
  });
});
