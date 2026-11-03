import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { useToastStore } from "../toasts/toastStore";
import { setTorrentBackend } from "./torrentBackend";
import { renderTorrents, spiedBackend } from "./torrentHarness";

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
});
afterEach(() => {
  restoreLayout();
  setTorrentBackend(null);
});

const entries = () => screen.queryAllByTestId("wish-entry");
const entryNamed = (name: string) => screen.findByRole("article", { name });

describe("the Wishlist", () => {
  it("lists the newest entries first and done ones last under Done", async () => {
    await renderTorrents("/torrents/wishlist");
    await entryNamed("Dune (2021)");
    expect(entries().map((entry) => entry.getAttribute("aria-label"))).toEqual([
      "Heat (1995)",
      "Dune (2021)",
      "Shōgun (2024)",
      "Severance (2022)",
      "Oppenheimer (2023)",
    ]);
    expect(await entryNamed("Dune (2021)")).toHaveTextContent(
      "2160p or better · Remux · HDR · up to",
    );
    expect(await entryNamed("Severance (2022)")).toHaveTextContent("Next new episode · S02E04");
    expect(await entryNamed("Shōgun (2024)")).toHaveTextContent("Next episode airs");
    expect(await entryNamed("Oppenheimer (2023)")).toHaveTextContent("Done: your library has it");
    expect(screen.getByRole("heading", { name: "Done" })).toBeInTheDocument();
  });

  it("says so when the Wishlist is empty", async () => {
    await renderTorrents("/torrents/wishlist", spiedBackend({ wishlist: "empty" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Your Wishlist is empty.");
  });

  it("adds a title with Add and lists it at once", async () => {
    const user = userEvent.setup();
    const backend = spiedBackend({ wishlist: "empty" });
    await renderTorrents("/torrents/wishlist", backend);
    await screen.findByRole("status");
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.type(screen.getByRole("combobox", { name: "Title to add" }), "alien");
    await user.click(await screen.findByRole("option", { name: /Alien \(1979\)/ }));
    const editor = await screen.findByTestId("wish-editor");
    await user.click(within(editor).getByRole("button", { name: "2160p" }));
    await user.click(within(editor).getByRole("button", { name: "Remux" }));
    await user.click(within(editor).getByRole("button", { name: "Add to Wishlist" }));
    const entry = await entryNamed("Alien (1979)");
    expect(entry).toHaveTextContent("2160p or better · Remux");
    expect(screen.queryByTestId("wish-editor")).toBeNull();
    expect(backend.wishlistCalls.wishlist).toContain("add:movie:8");
  });

  it("lists a title added from the address first and outlines it", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents/wishlist?add=movie:4");
    const editor = await screen.findByTestId("wish-editor");
    await user.click(within(editor).getByRole("button", { name: "Add to Wishlist" }));
    const entry = await entryNamed("Blade Runner (1982)");
    expect(entries()).toHaveLength(6);
    expect(entries()[0]).toBe(entry);
    expect(entry).toHaveAttribute("data-highlight", "true");
    expect(useToastStore.getState().toasts.map((toast) => toast.message)).toContain(
      "Added Blade Runner (1982) to your Wishlist",
    );
  });

  it("says Updated when the title and scope are on the Wishlist already", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents/wishlist?add=movie:3");
    const editor = await screen.findByTestId("wish-editor");
    await user.click(within(editor).getByRole("button", { name: "Add to Wishlist" }));
    await waitFor(() =>
      expect(useToastStore.getState().toasts.map((toast) => toast.message)).toContain(
        "Updated Heat (1995) on your Wishlist",
      ),
    );
    expect(entries()).toHaveLength(5);
  });

  it("checks now, announces new matches, and marks them seen", async () => {
    const user = userEvent.setup();
    const backend = spiedBackend();
    await renderTorrents("/torrents/wishlist", backend);
    await entryNamed("Heat (1995)");
    await user.click(screen.getByRole("button", { name: "Check now" }));
    await waitFor(() =>
      expect(
        within(screen.getByRole("article", { name: "Heat (1995)" })).getAllByTestId("wish-match"),
      ).toHaveLength(1),
    );
    expect(
      within(screen.getByRole("article", { name: "Heat (1995)" })).getByText("New"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(useToastStore.getState().toasts.map((toast) => toast.message)).toContain(
        "1 new Wishlist match",
      ),
    );
    await waitFor(() => expect(backend.wishlistCalls.wishlist).toContain("seen"));
    expect(await backend.wishlistNewCount()).toBe(0);
  });

  it("sends a match to the torrent app and dismisses another with Undo", async () => {
    const user = userEvent.setup();
    const backend = spiedBackend();
    await renderTorrents("/torrents/wishlist", backend);
    const dune = await entryNamed("Dune (2021)");
    const first = within(dune).getAllByTestId("wish-match")[0];
    if (first === undefined) throw new Error("no match");
    await user.click(within(first).getByRole("button", { name: "Send to torrent app" }));
    expect(backend.openMagnet).toHaveBeenCalledWith("magnet:?xt=urn:btih:d1a2");
    await waitFor(() => expect(backend.wishlistCalls.wishlist).toContain("match:1:d1a2:sent"));
    await waitFor(() =>
      expect(
        within(screen.getByRole("article", { name: "Dune (2021)" })).getAllByTestId(
          "wish-match",
        )[0],
      ).toHaveAttribute("data-state", "sent"),
    );
    const severance = await entryNamed("Severance (2022)");
    await user.click(within(severance).getByRole("button", { name: "Dismiss" }));
    await waitFor(() =>
      expect(
        within(screen.getByRole("article", { name: "Severance (2022)" })).queryAllByTestId(
          "wish-match",
        ),
      ).toHaveLength(0),
    );
    const toast = useToastStore.getState().toasts.find((entry) => entry.undo !== undefined);
    expect(toast?.message).toBe("Dismissed 1 release");
  });

  it("edits an entry in place and removes another", async () => {
    const user = userEvent.setup();
    const backend = spiedBackend();
    await renderTorrents("/torrents/wishlist", backend);
    const heat = await entryNamed("Heat (1995)");
    await user.click(within(heat).getByRole("button", { name: "Edit" }));
    const editor = within(heat).getByTestId("wish-editor");
    await user.click(within(editor).getByRole("button", { name: "1080p" }));
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(screen.getByRole("article", { name: "Heat (1995)" })).toHaveTextContent(
        "1080p or better",
      ),
    );
    await user.click(
      within(screen.getByRole("article", { name: "Shōgun (2024)" })).getByRole("button", {
        name: "Remove",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("article", { name: "Shōgun (2024)" })).toBeNull(),
    );
    expect(backend.wishlistCalls.wishlist).toContain("remove:4");
  });

  it("opens the editor for a title from the address with defaults from the copy held", async () => {
    await renderTorrents("/torrents/wishlist?add=movie:3");
    const editor = await screen.findByTestId("wish-editor");
    expect(within(editor).getByRole("button", { name: "2160p" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("says what the library holds in the editor and on the entry, linking to the title", async () => {
    await renderTorrents("/torrents/wishlist?add=movie:8");
    const editor = await screen.findByTestId("wish-editor");
    const line = await within(editor).findByTestId("wish-held");
    expect(line).toHaveTextContent("You already have this: 720p · Blu-ray");
    expect(within(line).getByRole("link")).toHaveAttribute("href", "/movies/8");

    const heat = await entryNamed("Heat (1995)");
    expect(within(heat).getByTestId("wish-held")).toHaveTextContent(
      "You already have this: 1080p · Blu-ray",
    );
    expect(heat).toHaveTextContent("2160p or better · HDR · from ");
    const done = await entryNamed("Oppenheimer (2023)");
    expect(within(done).queryByTestId("wish-held")).toBeNull();
  });

  it("keeps looking for a done entry and checks it at once", async () => {
    const user = userEvent.setup();
    const backend = spiedBackend();
    await renderTorrents("/torrents/wishlist", backend);
    const done = await entryNamed("Oppenheimer (2023)");
    await user.click(within(done).getByRole("button", { name: "Keep looking" }));
    await waitFor(() =>
      expect(screen.getByRole("article", { name: "Oppenheimer (2023)" })).toHaveAttribute(
        "data-done",
        "false",
      ),
    );
    expect(screen.queryByRole("heading", { name: "Done" })).toBeNull();
    expect(backend.wishlistCalls.wishlist).toContain("keep:5");
    await waitFor(() => expect(backend.wishlistCalls.wishlist).toContain("check:true"));
    expect(useToastStore.getState().toasts.map((toast) => toast.message)).toContain(
      "Looking for Oppenheimer (2023) again",
    );
  });

  it("takes a size range and refuses a smallest size above the largest", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents/wishlist?add=movie:4");
    const editor = await screen.findByTestId("wish-editor");
    expect(within(editor).queryByTestId("wish-held")).toBeNull();
    await user.type(within(editor).getByRole("textbox", { name: "Smallest size" }), "60");
    await user.type(within(editor).getByRole("textbox", { name: "Largest size" }), "20");
    await user.click(within(editor).getByRole("button", { name: "Add to Wishlist" }));
    expect(await within(editor).findByRole("alert")).toHaveTextContent(
      "The smallest size must not be above the largest size.",
    );
    await user.clear(within(editor).getByRole("textbox", { name: "Smallest size" }));
    await user.type(within(editor).getByRole("textbox", { name: "Smallest size" }), "5");
    await user.click(within(editor).getByRole("button", { name: "Add to Wishlist" }));
    expect(await entryNamed("Blade Runner (1982)")).toHaveTextContent(/5–20\sGB/);
  });
});

describe("Add to Wishlist on the search page", () => {
  it("opens the Wishlist editor for the picked title", async () => {
    const user = userEvent.setup();
    const { router } = await renderTorrents("/torrents?title=movie:4");
    await user.click(await screen.findByRole("button", { name: "Add to Wishlist" }));
    await waitFor(() => expect(router.state.location.pathname).toBe("/torrents/wishlist"));
    expect(decodeURIComponent(router.state.location.searchStr)).toContain("add=movie:4");
  });

  it("shows On your Wishlist for a title that is there", async () => {
    await renderTorrents("/torrents?title=movie:2");
    expect(await screen.findByRole("button", { name: "On your Wishlist" })).toBeInTheDocument();
  });
});
