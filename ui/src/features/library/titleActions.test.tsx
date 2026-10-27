import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { ContextMenu } from "../../components/ContextMenu";
import { LibrarySelectionBar } from "./LibrarySelectionBar";
import { TitleContextMenu } from "./TitleContextMenu";
import { BAR_ORDER, HEADER_ORDER, MENU_GROUPS, MORE_ORDER, TITLE_ACTIONS } from "./titleActions";
import { useLibraryActions } from "./useLibraryActions";
import type { TitleTarget } from "./useLibraryActions";

const target = (id: string, title: string): TitleTarget => ({
  id,
  kind: "movie",
  title,
  folder: `D:/Movies/${title}`,
  filesMissing: false,
});

function Page({ targets }: { targets: readonly TitleTarget[] }) {
  const actions = useLibraryActions(targets);
  return (
    <div>
      <ContextMenu menu={<TitleContextMenu actions={actions} />}>
        <div data-testid="area">area</div>
      </ContextMenu>
      <LibrarySelectionBar
        actions={actions}
        count={targets.length}
        size="186.2 GB"
        onClear={() => undefined}
      />
    </div>
  );
}

async function renderPage(targets: readonly TitleTarget[]) {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/",
        component: () => <Page targets={targets} />,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(<RouterProvider router={router} />);
  await screen.findByRole("toolbar", { name: "Bulk actions" });
}

const four = [target("1", "Heat"), target("2", "Alien"), target("3", "Dune"), target("4", "Jaws")];

const names = (items: HTMLElement[]) => items.map((item) => item.textContent?.trim() ?? "");

describe("title actions", () => {
  it("lists the bar as Rename, Scrape, Artwork, Move, Delete and More with the count and size", async () => {
    await renderPage(four);
    const bar = screen.getByRole("toolbar", { name: "Bulk actions" });
    expect(within(bar).getByText("4 selected")).toBeInTheDocument();
    expect(within(bar).getByText("186.2 GB")).toBeInTheDocument();
    const buttons = within(bar)
      .getAllByRole("button")
      .map((b) => b.textContent?.trim());
    expect(buttons).toEqual(["Rename", "Scrape", "Artwork", "Move", "Delete", "More", ""]);
  });

  it("keeps the bar, More and the menu in the one order, with the same names", async () => {
    const user = userEvent.setup();
    await renderPage(four);
    const bar = screen.getByRole("toolbar", { name: "Bulk actions" });

    await user.click(within(bar).getByRole("button", { name: "More" }));
    const more = await screen.findAllByRole("menuitem");
    const moreNames = names(more);
    expect(moreNames).toEqual([
      "Rewrite NFO",
      "Reload media info",
      "Edit details",
      "Remove from library (keep files)",
      "Ignore this folder",
      "Delete permanently…Shift Del",
    ]);
    expect(more).toHaveLength(MORE_ORDER.length);
    await user.keyboard("{Escape}");

    await user.pointer({ target: screen.getByTestId("area"), keys: "[MouseRight]" });
    const menu = await screen.findAllByRole("menuitem");
    const menuNames = names(menu);
    expect(menuNames).toEqual([
      "OpenEnter",
      "Play",
      "Open folder",
      "Rename 4 titlesF2",
      "Scrape 4 titles",
      "Artwork…",
      "Move to…",
      "Rewrite NFO",
      "Reload media info",
      "Edit details",
      "Remove from library (keep files)",
      "Ignore this folder",
      "Delete to Recycle BinDel",
      "Delete permanently…Shift Del",
    ]);
    // Every action of the bar and of More is in the menu, once.
    expect(menu).toHaveLength(MENU_GROUPS.flat().length);
    for (const id of [...BAR_ORDER, ...MORE_ORDER]) expect(MENU_GROUPS.flat()).toContain(id);
  });

  it("says why an action that works on one title is unavailable for several", async () => {
    const user = userEvent.setup();
    await renderPage(four);
    await user.pointer({ target: screen.getByTestId("area"), keys: "[MouseRight]" });
    await screen.findAllByRole("menuitem");
    expect(document.querySelector('[data-action="open"]')).toHaveAttribute("aria-disabled", "true");
  });

  it("names one title without a count and enables Open", async () => {
    const user = userEvent.setup();
    await renderPage([target("1", "Heat")]);
    await user.pointer({ target: screen.getByTestId("area"), keys: "[MouseRight]" });
    expect(await screen.findByRole("menuitem", { name: /^Rename/ })).toHaveTextContent(
      /^RenameF2$/,
    );
    expect(screen.getByRole("menuitem", { name: /^Scrape$/ })).toBeInTheDocument();
    expect(document.querySelector('[data-action="open"]')).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("shares icons, names and hints with the title page header", () => {
    // The page header reads the same list: its six buttons exist in the registry.
    for (const id of HEADER_ORDER) expect(TITLE_ACTIONS[id].icon).toBeDefined();
    expect(HEADER_ORDER).toEqual(["play", "scrape", "rename", "move", "open-folder", "artwork"]);
    expect(TITLE_ACTIONS.rename.hint).toBe("F2");
    expect(TITLE_ACTIONS.delete.hint).toBe("Del");
    expect(TITLE_ACTIONS["delete-permanently"].hint).toBe("Shift Del");
  });
});
