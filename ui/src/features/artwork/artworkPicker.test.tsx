import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { useNotificationStore } from "../notifications/notificationStore";
import { handleEscape } from "../shell/escStack";
import { useToastStore } from "../toasts/toastStore";
import { ArtworkPage } from "./ArtworkPage";
import { setArtworkBackend } from "./artworkBackend";
import { validateArtworkSearch } from "./artworkSearch";
import { createSyntheticArtworkBackend } from "./syntheticArtwork";

function renderPicker(search = "?ids=1") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/artwork",
        validateSearch: validateArtworkSearch,
        component: ArtworkPage,
      }),
      createRoute({
        getParentRoute: () => root,
        path: "/library",
        component: () => <p>Library page</p>,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/library", `/artwork${search}`] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  return router;
}

let backend: ReturnType<typeof createSyntheticArtworkBackend>;

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
  backend = createSyntheticArtworkBackend();
  setArtworkBackend(backend);
});
afterEach(() => {
  restoreLayout();
  setArtworkBackend(null);
});

// Plain DOM query: a role query over every tile is the slow part of these tests under load.
const tiles = async (): Promise<HTMLElement[]> => {
  const list = await screen.findByRole("listbox", { name: "Images" });
  return Array.from(list.querySelectorAll<HTMLElement>('[role="option"]'));
};
const waitTiles = async (count: number) => {
  await waitFor(async () => expect((await tiles()).length).toBeGreaterThanOrEqual(count));
};
const preview = () => screen.getByRole("complementary", { name: "Preview" });

describe("the artwork picker page", () => {
  it("is a page with a header naming the title, Done with its Esc hint and no dialog", async () => {
    renderPicker();
    expect(
      await screen.findByRole("heading", { name: "Artwork · Dune: Part Two (2024)" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Done/ })).toHaveTextContent("Esc");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("lists the types with counts and opens on the one the address names", async () => {
    renderPicker("?ids=1&type=fanart");
    const strip = await screen.findByRole("navigation", { name: "Artwork type" });
    await waitFor(() => expect(within(strip).getByRole("button", { name: /^Logo/ })).toBeVisible());
    expect(within(strip).getByRole("button", { name: /^Fanart/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
    // Every type has its count once its search answered.
    await waitFor(() =>
      expect(within(strip).getByRole("button", { name: /^Poster\s*\d+/ })).toBeVisible(),
    );
    for (const name of [
      /^Poster/,
      /^Fanart/,
      /^Logo/,
      /^Clear art/,
      /^Banner/,
      /^Disc/,
      /^Thumb/,
      /^Key art/,
    ]) {
      expect(within(strip).getByRole("button", { name })).toBeInTheDocument();
    }
  });

  it("takes the counts of the other types from one lookup and searches only the type on screen", async () => {
    const searched: string[] = [];
    const counting = createSyntheticArtworkBackend({
      counts: [
        { slot: "poster", count: 14, current: null },
        { slot: "fanart", count: 9, current: null },
        { slot: "clearlogo", count: 0, current: null },
      ],
    });
    const base = counting.searchTransport;
    counting.searchTransport = (onStart) => {
      const inner = base(onStart);
      return {
        ...inner,
        start: (request, onEvent) => {
          searched.push(String(request.params?.slot));
          inner.start(request, onEvent);
        },
      };
    };
    setArtworkBackend(counting);
    renderPicker();
    const strip = await screen.findByRole("navigation", { name: "Artwork type" });
    await waitFor(() =>
      expect(within(strip).getByRole("button", { name: /^Fanart\s*9/ })).toBeVisible(),
    );
    expect(within(strip).getByRole("button", { name: /^Poster/ })).toBeInTheDocument();
    expect(within(strip).queryByRole("button", { name: /^Banner/ })).toBeNull();
    await waitTiles(4);
    // Only the poster, the type on screen, was searched.
    expect(searched).toEqual(["poster"]);
  });

  it("shows the current image pinned first with its tag, and the language chips with counts", async () => {
    renderPicker();
    await waitTiles(4);
    const first = (await tiles())[0] as HTMLElement;
    expect(first).toHaveAttribute("data-current", "true");
    expect(within(first).getByText("Current")).toBeInTheDocument();
    const chips = screen.getByRole("group", { name: "Language" });
    expect(within(chips).getByRole("button", { name: /^All\s*\d+/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(within(chips).getByRole("button", { name: /^English\s*\d+/ })).toBeInTheDocument();
  });

  it("previews a click without writing, and Use this writes it with an Undo toast", async () => {
    const user = userEvent.setup();
    renderPicker();
    await waitTiles(4);
    const target = (await tiles())[2] as HTMLElement;
    await user.click(target);
    expect(target).toHaveAttribute("aria-selected", "true");
    // The preview shows its size, language, source and the file name it is saved as.
    const panel = within(preview());
    expect(panel.getByText(/^\d+×\d+$/)).toBeInTheDocument();
    expect(panel.getByText("poster.jpg")).toBeInTheDocument();
    expect(panel.getByText(/TMDB|Fanart\.tv|TheTVDB/)).toBeInTheDocument();
    expect(backend.placed).toHaveLength(0);

    await user.click(panel.getByRole("button", { name: "Use this" }));
    await waitFor(() => expect(backend.placed).toHaveLength(1));
    expect(backend.placed[0]).toMatchObject({ itemId: "1", slot: "poster" });
    const toast = useToastStore
      .getState()
      .toasts.find((entry) => /^Changed poster/.test(entry.message));
    expect(toast?.message).toBe("Changed poster · Dune: Part Two (2024)");
    expect(toast?.undo).toBeDefined();
    await act(async () => {
      await toast?.undo?.();
    });
    expect(backend.undone).toHaveLength(1);
    // The picked image is the current one at once.
    await waitFor(async () =>
      expect((await tiles())[0]).toHaveAttribute(
        "data-art-candidate",
        backend.placed[0]?.sources[0] ?? "",
      ),
    );
  });

  it("applies with Enter, and keeps the current image as it is", async () => {
    const user = userEvent.setup();
    renderPicker();
    await waitTiles(4);
    const list = await screen.findByRole("listbox", { name: "Images" });
    await user.click((await tiles())[1] as HTMLElement);
    list.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(backend.placed).toHaveLength(1));

    await waitTiles(4);
    list.focus();
    await user.keyboard("{ArrowRight}{Enter}");
    await waitFor(() => expect(backend.placed).toHaveLength(2));
    // The current image cannot be used again: Use this says why.
    await user.click((await tiles())[0] as HTMLElement);
    expect(within(preview()).getByRole("button", { name: "Use this" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  describe("the large preview", () => {
    const lightbox = () => screen.queryByTestId("art-lightbox");

    it("opens from a double click without choosing or writing anything, and any click closes it", async () => {
      const user = userEvent.setup();
      renderPicker();
      await waitTiles(4);
      const list = await screen.findByRole("listbox", { name: "Images" });
      await user.dblClick((await tiles())[2] as HTMLElement);
      const open = await screen.findByTestId("art-lightbox");
      // Under the picture: the type, the size and where it came from.
      expect(open).toHaveAccessibleName(/^Poster · \d+×\d+ · .+/);
      expect(within(open).queryByRole("button")).toBeNull();
      expect(backend.placed).toHaveLength(0);
      await user.click(open);
      expect(lightbox()).toBeNull();
      expect(backend.placed).toHaveLength(0);
      expect(list).toBeInTheDocument();
    });

    it("opens with Space on the chosen image and closes with Esc without leaving the picker", async () => {
      const user = userEvent.setup();
      const router = renderPicker();
      await waitTiles(4);
      const list = await screen.findByRole("listbox", { name: "Images" });
      await user.click((await tiles())[1] as HTMLElement);
      list.focus();
      await user.keyboard(" ");
      expect(await screen.findByTestId("art-lightbox")).toBeInTheDocument();
      await user.keyboard("{Escape}");
      expect(lightbox()).toBeNull();
      expect(router.state.location.pathname).toBe("/artwork");
      // Focus is where it was: the grid answers the keys again, with the choice kept.
      expect(list).toHaveFocus();
      expect((await tiles())[1]).toHaveAttribute("aria-selected", "true");
    });

    it("flicks through the images of the type with the arrow keys, never changing the choice", async () => {
      const user = userEvent.setup();
      renderPicker();
      await waitTiles(4);
      const list = await screen.findByRole("listbox", { name: "Images" });
      await user.click((await tiles())[1] as HTMLElement);
      list.focus();
      await user.keyboard(" ");
      const first = (await screen.findByTestId("art-lightbox"))
        .querySelector("img")
        ?.getAttribute("src");
      await user.keyboard("{ArrowRight}");
      const second = screen.getByTestId("art-lightbox").querySelector("img")?.getAttribute("src");
      expect(second).not.toBe(first);
      await user.keyboard("{ArrowLeft}");
      expect(screen.getByTestId("art-lightbox").querySelector("img")?.getAttribute("src")).toBe(
        first,
      );
      await user.keyboard("{Escape}");
      expect((await tiles())[1]).toHaveAttribute("aria-selected", "true");
      expect(backend.placed).toHaveLength(0);
    });

    it("opens from the expand button of the preview pane", async () => {
      const user = userEvent.setup();
      renderPicker();
      await waitTiles(4);
      await user.click((await tiles())[1] as HTMLElement);
      await user.click(within(preview()).getByRole("button", { name: "Expand preview" }));
      expect(await screen.findByTestId("art-lightbox")).toBeInTheDocument();
      await user.click(document.body);
      expect(lightbox()).toBeNull();
    });

    it("appears at once, without the grow animation, under reduced motion", async () => {
      document.documentElement.dataset.motion = "reduced";
      try {
        const user = userEvent.setup();
        renderPicker();
        await waitTiles(4);
        await user.dblClick((await tiles())[1] as HTMLElement);
        const open = await screen.findByTestId("art-lightbox");
        expect(open.style.animation).toBe("none");
        expect(open.querySelector("figure")?.style.animation).toBe("none");
      } finally {
        delete document.documentElement.dataset.motion;
      }
    });
  });

  it("filters by language and sorts by size", async () => {
    const user = userEvent.setup({ delay: null });
    renderPicker();
    await waitTiles(4);
    const chips = screen.getByRole("group", { name: "Language" });
    const english = within(chips).getByRole("button", { name: /^English/ });
    const count = Number(/(\d+)\s*$/.exec(english.textContent ?? "")?.[1]);
    await user.click(english);
    await waitFor(async () => expect((await tiles()).length).toBeLessThanOrEqual(count + 1));
    for (const tile of await tiles()) {
      if (tile.getAttribute("data-current") === "true") continue;
      const labels = Array.from(tile.querySelectorAll("*")).map((node) => node.textContent);
      expect(labels).toContain("EN");
    }
    await user.click(within(chips).getByRole("button", { name: /^All/ }));

    await user.click(screen.getByRole("button", { name: /^Sort: Votes/ }));
    await user.click(await screen.findByRole("menuitemradio", { name: "Size" }));
    expect(screen.getByRole("button", { name: /^Sort: Size/ })).toBeInTheDocument();
    const sizes = (await tiles())
      .filter((tile) => tile.getAttribute("data-current") !== "true")
      .map((tile) => {
        const [w, h] = (/(\d+)×(\d+)/.exec(tile.getAttribute("aria-label") ?? "") ?? [])
          .slice(1)
          .map(Number);
        return (w ?? 0) * (h ?? 0);
      });
    expect(sizes.slice(0, 3)).toEqual([...sizes.slice(0, 3)].sort((a, b) => b - a));
  });

  it("shows a dropped picture in the preview as the new poster and writes it only on Use this", async () => {
    const user = userEvent.setup();
    renderPicker();
    await waitTiles(2);
    const file = new File(["x"], "my-poster.jpg", { type: "image/jpeg" });
    const target = screen.getByRole("navigation", { name: "Artwork type" })
      .parentElement as HTMLElement;
    fireEvent.dragOver(target, { dataTransfer: { types: ["Files"], files: [file] } });
    fireEvent.drop(target, { dataTransfer: { types: ["Files"], files: [file] } });
    const panel = within(preview());
    expect(await panel.findByText("my-poster.jpg")).toBeInTheDocument();
    expect(panel.getByText("A file on this PC")).toBeInTheDocument();
    expect(panel.getByText("poster.jpg")).toBeInTheDocument();
    expect(backend.placed).toHaveLength(0);

    await user.click(panel.getByRole("button", { name: "Use this" }));
    await waitFor(() => expect(backend.placed).toHaveLength(1));
    expect(backend.placed[0]?.sources).toEqual(["my-poster.jpg"]);
    expect(
      useToastStore.getState().toasts.some((entry) => /^Changed poster/.test(entry.message)),
    ).toBe(true);
  });

  it("does not take a file that is not a picture", async () => {
    renderPicker();
    await waitTiles(2);
    const file = new File(["x"], "notes.txt", { type: "text/plain" });
    const target = screen.getByRole("navigation", { name: "Artwork type" })
      .parentElement as HTMLElement;
    fireEvent.drop(target, { dataTransfer: { types: ["Files"], files: [file] } });
    await waitFor(() =>
      expect(
        useNotificationStore
          .getState()
          .items.some((item) => /notes\.txt is not a picture/.test(item.title)),
      ).toBe(true),
    );
    expect(within(preview()).queryByText("notes.txt")).toBeNull();
  });

  it("takes a pasted link into the preview and writes it on Use this", async () => {
    const user = userEvent.setup();
    renderPicker();
    await waitTiles(2);
    await user.click(screen.getByRole("button", { name: "From a link…" }));
    await user.type(
      screen.getByRole("textbox", { name: "Image address" }),
      "https://img.example/mine.jpg",
    );
    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(within(preview()).getByText("https://img.example/mine.jpg")).toBeInTheDocument();
    await user.click(within(preview()).getByRole("button", { name: "Use this" }));
    await waitFor(() => expect(backend.placed).toHaveLength(1));
    expect(backend.placed[0]?.sources).toEqual(["https://img.example/mine.jpg"]);
  });

  it("switches type from the strip and keeps nothing chosen across types", async () => {
    const user = userEvent.setup();
    renderPicker();
    await waitTiles(4);
    await user.click((await tiles())[2] as HTMLElement);
    await user.click(screen.getByRole("button", { name: /^Fanart/ }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^Fanart/ })).toHaveAttribute(
        "aria-current",
        "true",
      ),
    );
    await waitTiles(2);
    expect(within(preview()).queryByText("poster.jpg")).toBeNull();
    for (const tile of await tiles()) expect(tile).toHaveAttribute("aria-selected", "false");
  });

  it("says that one provider failed in one line with Retry, and the others stay", async () => {
    const user = userEvent.setup();
    renderPicker();
    await waitTiles(4);
    expect(await screen.findByText(/TheTVDB could not answer/)).toBeInTheDocument();
    const before = (await tiles()).length;
    await user.click(screen.getByRole("button", { name: "Retry TheTVDB" }));
    await waitFor(async () => expect((await tiles()).length).toBeGreaterThan(before));
  });

  it("closes with Esc and with Done, back to where it was opened, writing nothing", async () => {
    const user = userEvent.setup();
    const router = renderPicker();
    await waitTiles(2);
    await act(async () => {
      expect(handleEscape({ defaultPrevented: false, target: document.body })).toBe(true);
    });
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    await act(async () => router.history.forward());
    await user.click(await screen.findByRole("button", { name: /^Done/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(backend.placed).toHaveLength(0);
  });
});

describe("artwork for several titles", () => {
  const BATCH = "?ids=1%2C2%2C3&type=fanart";
  const counts = () => screen.getByText(/changed · \d+ skipped/);
  /** The batch page is ready once the first title's name and picker are there. */
  const ready = async (step: string) => {
    await screen.findByText(step);
    await screen.findByRole("complementary", { name: "Preview" });
  };

  it("steps through the titles: Title 1 of 3, the name, the counts and a strip of the titles", async () => {
    renderPicker(BATCH);
    await ready("Artwork · Title 1 of 3");
    expect(screen.getByRole("heading", { name: "Dune: Part Two (2024)" })).toBeInTheDocument();
    expect(counts()).toHaveTextContent("0 changed · 0 skipped");
    const strip = screen.getByRole("group", { name: "Selected titles" });
    const tilesOf = within(strip).getAllByRole("button");
    expect(tilesOf).toHaveLength(3);
    expect(tilesOf[0]).toHaveAttribute("aria-current", "true");
    expect(screen.getByRole("button", { name: "Previous title" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Done" })).toBeInTheDocument();
    // The picker opens on the type the address names.
    expect(screen.getByRole("button", { name: /^Fanart/ })).toHaveAttribute("aria-current", "true");
  });

  it("applies the choice and goes on to the next title on the same type", async () => {
    const user = userEvent.setup();
    renderPicker(BATCH);
    await ready("Artwork · Title 1 of 3");
    await waitTiles(3);
    await user.click((await tiles())[1] as HTMLElement);
    await user.click(within(preview()).getByRole("button", { name: /^Use this and go to next/ }));
    expect(await screen.findByText("Artwork · Title 2 of 3")).toBeInTheDocument();
    await waitFor(() => expect(backend.placed).toHaveLength(1));
    expect(backend.placed[0]).toMatchObject({ itemId: "1", slot: "fanart" });
    await waitFor(() => expect(counts()).toHaveTextContent("1 changed · 0 skipped"));
    // The type stays fanart for the next title, and the changed title is marked in the strip.
    expect(screen.getByRole("button", { name: /^Fanart/ })).toHaveAttribute("aria-current", "true");
    const strip = screen.getByRole("group", { name: "Selected titles" });
    expect(within(strip).getAllByRole("button")[0]).toHaveAttribute("data-done", "true");
  });

  it("goes on with Enter after a click chose the image", async () => {
    const user = userEvent.setup();
    renderPicker(BATCH);
    await ready("Artwork · Title 1 of 3");
    await waitTiles(3);
    await user.click((await tiles())[2] as HTMLElement);
    await user.keyboard("{Enter}");
    expect(await screen.findByText("Artwork · Title 2 of 3")).toBeInTheDocument();
    await waitFor(() => expect(backend.placed).toHaveLength(1));
  });

  it("uses an image and stays on the title with Use this", async () => {
    const user = userEvent.setup();
    renderPicker(BATCH);
    await ready("Artwork · Title 1 of 3");
    await waitTiles(3);
    await user.click((await tiles())[1] as HTMLElement);
    await user.click(within(preview()).getByRole("button", { name: "Use this" }));
    await waitFor(() => expect(backend.placed).toHaveLength(1));
    expect(screen.getByText("Artwork · Title 1 of 3")).toBeInTheDocument();
    await waitFor(() => expect(counts()).toHaveTextContent("1 changed · 0 skipped"));
  });

  it("skips a title without picking anything for it", async () => {
    const user = userEvent.setup();
    renderPicker(BATCH);
    await ready("Artwork · Title 1 of 3");
    await user.click(within(preview()).getByRole("button", { name: "Skip title" }));
    expect(await screen.findByText("Artwork · Title 2 of 3")).toBeInTheDocument();
    expect(counts()).toHaveTextContent("0 changed · 1 skipped");
    expect(backend.placed).toHaveLength(0);
  });

  it("moves between the titles with the arrows and finishes with Done", async () => {
    const user = userEvent.setup();
    const router = renderPicker(BATCH);
    await ready("Artwork · Title 1 of 3");
    await user.click(screen.getByRole("button", { name: "Next title" }));
    expect(await screen.findByText("Artwork · Title 2 of 3")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Previous title" }));
    expect(await screen.findByText("Artwork · Title 1 of 3")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
    expect(backend.placed).toHaveLength(0);
  });

  it("finishes after the last title", async () => {
    const user = userEvent.setup();
    const router = renderPicker("?ids=1%2C2");
    await ready("Artwork · Title 1 of 2");
    await user.click(within(preview()).getByRole("button", { name: "Skip title" }));
    await ready("Artwork · Title 2 of 2");
    await user.click(within(preview()).getByRole("button", { name: "Skip title" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/library"));
  });
});

describe("artwork of a collection", () => {
  it("changes only the poster and the fanart", async () => {
    renderPicker("?collection=1");
    expect(await screen.findByRole("heading", { name: /^Artwork · / })).toBeInTheDocument();
    const strip = await screen.findByRole("navigation", { name: "Artwork type" });
    await waitFor(() => expect(within(strip).getAllByRole("button")).toHaveLength(2));
    expect(within(strip).getByRole("button", { name: /^Poster/ })).toBeInTheDocument();
    expect(within(strip).getByRole("button", { name: /^Fanart/ })).toBeInTheDocument();
  });
});
