import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { DEFAULT_SORT, readSort, sortReleases, tierCounts } from "./releaseSort";
import { setTorrentBackend } from "./torrentBackend";
import { backendWith, release, renderTorrents, spiedBackend } from "./torrentHarness";

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
});
afterEach(() => {
  restoreLayout();
  setTorrentBackend(null);
});

const grid = () => screen.findByRole("grid", { name: "Releases" });
const dataRows = async () => (await within(await grid()).findAllByRole("row")).slice(1);
const names = async () =>
  (await dataRows()).map((row) => within(row).getAllByRole("gridcell")[1]?.textContent ?? "");

describe("the Torrents page search", () => {
  it("shows the first-visit notice and searches nothing until it is accepted", async () => {
    const backend = spiedBackend();
    const user = userEvent.setup();
    await renderTorrents("/torrents?q=dune", backend, false);
    expect(await screen.findByTestId("torrent-notice")).toBeVisible();
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(backend.titleStarts).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Got it" }));
    expect(await screen.findByRole("combobox", { name: "Search torrents" })).toBeVisible();
  });

  it("suggests titles after the typing pause and searches no releases while typing", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    const field = await screen.findByRole("combobox", { name: "Search torrents" });
    expect(screen.getByText("Type a movie or show to see its releases.")).toBeVisible();
    await user.type(field, "blade runner");
    // No title lookup while the typing goes on (the pause is short but real).
    expect(backend.titleStarts).toHaveLength(0);
    const panel = await screen.findByRole(
      "listbox",
      { name: "Title suggestions" },
      { timeout: 3000 },
    );
    expect(backend.titleStarts.at(-1)?.query).toBe("blade runner");
    // Titles, not torrents: poster rows with the type, the owned one marked.
    const owned = await within(panel).findByRole("option", { name: /Blade Runner 2049 \(2017\)/ });
    expect(owned).toHaveTextContent("In your library");
    expect(within(panel).getAllByRole("option").length).toBeGreaterThan(1);
    // Give a wrongly started search time to show itself.
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(backend.releaseStarts).toHaveLength(0);
    expect(screen.queryByRole("grid", { name: "Releases" })).toBeNull();
    expect(screen.getByText("Type a movie or show to see its releases.")).toBeVisible();
  });

  it("starts the release search of exactly the picked title, and shows it as a chip", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    await user.type(await screen.findByRole("combobox"), "blade runner");
    await user.click(await screen.findByRole("option", { name: /Blade Runner 2049 \(2017\)/ }));
    await waitFor(() => expect(backend.releaseStarts.length).toBeGreaterThan(0), { timeout: 3000 });
    expect(backend.releaseStarts.at(-1)?.params).toMatchObject({ kind: "movie", tmdbId: 1 });
    const chip = await screen.findByTestId("picked-title");
    expect(chip).toHaveTextContent("Blade Runner 2049 (2017)");
    expect(chip).toHaveTextContent("Movie");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(screen.getByRole("combobox")).toHaveValue("Blade Runner 2049 (2017)");
    expect((await dataRows()).length).toBeGreaterThan(3);
    expect(await screen.findByText(/^\d+ results? · \d+ sources?$/)).toBeVisible();
  });

  it("takes the best suggestion with Enter when none is highlighted", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    await user.type(await screen.findByRole("combobox"), "heat");
    await screen.findByRole("option", { name: /Heat \(1995\)/ });
    expect(backend.releaseStarts).toHaveLength(0);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(backend.releaseStarts.length).toBeGreaterThan(0), { timeout: 3000 });
    expect(await screen.findByTestId("picked-title")).toHaveTextContent("Heat (1995)");
  });

  it("picks with the arrow keys and Enter", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    await user.type(await screen.findByRole("combobox"), "blade");
    const options = await screen.findAllByRole("option");
    expect(options.length).toBeGreaterThan(1);
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(screen.getByRole("combobox")).toHaveAttribute("aria-activedescendant", options[1]?.id);
    const second = options[1]?.getAttribute("aria-label") ?? "";
    await user.keyboard("{Enter}");
    await waitFor(() => expect(backend.releaseStarts.length).toBeGreaterThan(0), { timeout: 3000 });
    expect((await screen.findByTestId("picked-title")).textContent).toContain(
      second.split(",")[0] ?? "",
    );
  });

  it("clears the results when the title is changed with the chip's ×", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents?title=movie:1", backend);
    expect((await dataRows()).length).toBeGreaterThan(3);
    await user.click(screen.getByRole("button", { name: "Change title" }));
    await waitFor(() => expect(screen.queryByRole("grid", { name: "Releases" })).toBeNull());
    expect(screen.queryByTestId("picked-title")).toBeNull();
    expect(screen.getByRole("combobox")).toHaveValue("");
    expect(screen.getByRole("combobox")).toHaveFocus();
    expect(screen.getByText("Type a movie or show to see its releases.")).toBeVisible();
  });

  it("drops the title and its results when the field is edited after a pick", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    const { router } = await renderTorrents("/torrents", backend);
    await user.type(await screen.findByRole("combobox"), "heat");
    await user.click(await screen.findByRole("option", { name: /Heat \(1995\)/ }));
    await dataRows();
    await waitFor(() => expect(router.state.location.search).toMatchObject({ title: "movie:3" }));
    await user.type(screen.getByRole("combobox"), "x");
    await waitFor(() => expect(screen.queryByRole("grid", { name: "Releases" })).toBeNull());
    expect(screen.queryByTestId("picked-title")).toBeNull();
    await waitFor(() => expect(router.state.location.search).toEqual({}));
  });

  it("opens with the title of the address and searches its releases at once", async () => {
    const backend = spiedBackend({ failing: [] });
    await renderTorrents("/torrents?title=movie:1", backend);
    expect((await dataRows()).length).toBeGreaterThan(3);
    expect(backend.titleStarts).toHaveLength(0);
    expect(await screen.findByDisplayValue("Blade Runner 2049 (2017)")).toBeVisible();
  });

  it("says when nothing was found", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents", spiedBackend({ failing: [] }));
    await user.type(await screen.findByRole("combobox"), "zzzzqqqq");
    expect(
      await screen.findByText("Nothing found for “zzzzqqqq”", {}, { timeout: 4000 }),
    ).toBeVisible();
  });
});

describe("the resolution chips", () => {
  it("count the releases of each resolution and list only the chosen one", async () => {
    const user = userEvent.setup();
    await renderTorrents(
      "/torrents?title=movie:1",
      backendWith([
        release({ id: "a1", name: "A.2160p", resolution: "2160p" }),
        release({ id: "a2", name: "B.2160p", resolution: "2160p" }),
        release({ id: "b1", name: "C.1080p", resolution: "1080p" }),
        release({ id: "c1", name: "D.480p", resolution: "480p" }),
      ]),
    );
    const chips = await screen.findByRole("group", { name: "Resolution" });
    await waitFor(() =>
      expect(within(chips).getByRole("button", { name: /^All\s*4/ })).toBeVisible(),
    );
    expect(within(chips).getByRole("button", { name: /^2160p\s*2/ })).toBeVisible();
    expect(within(chips).getByRole("button", { name: /^1080p\s*1/ })).toBeVisible();
    expect(within(chips).getByRole("button", { name: /^720p\s*0/ })).toBeVisible();
    // 480p and unknown count as SD.
    expect(within(chips).getByRole("button", { name: /^SD\s*1/ })).toBeVisible();
    await user.click(within(chips).getByRole("button", { name: /^2160p/ }));
    expect(await names()).toEqual(["A.2160p", "B.2160p"]);
    expect(within(chips).getByRole("button", { name: /^2160p/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});

describe("sorting the releases", () => {
  const releases = [
    release({ id: "a", name: "A", seeders: 5, size: 30_000_000_000, published: 3 }),
    release({ id: "b", name: "B", seeders: 500, size: 10_000_000_000, published: 1 }),
    release({ id: "c", name: "C", seeders: null, size: null, published: null }),
  ];

  it("starts with the most seeders first and sorts by a header click", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents?title=movie:1", backendWith(releases));
    expect(await names()).toEqual(["B", "A", "C"]);
    expect(screen.getByRole("columnheader", { name: "Seeders" })).toHaveAttribute(
      "aria-sort",
      "descending",
    );
    await user.click(
      within(screen.getByRole("columnheader", { name: "Size" })).getByRole("button"),
    );
    // Ascending by size; a release without a size stays last.
    expect(await names()).toEqual(["B", "A", "C"]);
    expect(screen.getByRole("columnheader", { name: "Size" })).toHaveAttribute(
      "aria-sort",
      "ascending",
    );
  });

  it("remembers the sort when the page is opened again", async () => {
    const user = userEvent.setup();
    const first = await renderTorrents("/torrents?title=movie:1", backendWith(releases));
    await user.click(
      within(screen.getByRole("columnheader", { name: "Size" })).getByRole("button"),
    );
    await user.click(
      within(screen.getByRole("columnheader", { name: "Size" })).getByRole("button"),
    );
    expect(readSort()).toEqual({ column: "size", direction: "desc" });
    first.unmount();
    await renderTorrents("/torrents?title=movie:1", backendWith(releases));
    await dataRows();
    expect(screen.getByRole("columnheader", { name: "Size" })).toHaveAttribute(
      "aria-sort",
      "descending",
    );
    expect(await names()).toEqual(["A", "B", "C"]);
  });

  it("puts a release without the figure last in either direction", () => {
    const sourceOf = () => "";
    expect(sortReleases(releases, DEFAULT_SORT, sourceOf).map((entry) => entry.name)).toEqual([
      "B",
      "A",
      "C",
    ]);
    expect(
      sortReleases(releases, { column: "seeders", direction: "asc" }, sourceOf).map(
        (entry) => entry.name,
      ),
    ).toEqual(["A", "B", "C"]);
    // The age column: the newest release is the youngest.
    expect(
      sortReleases(releases, { column: "age", direction: "asc" }, sourceOf).map((e) => e.name),
    ).toEqual(["A", "B", "C"]);
  });

  it("falls back to the default for a damaged saved sort", () => {
    window.localStorage.setItem("cinevault.torrents.sort", '{"column":"name","direction":"up"}');
    expect(readSort()).toEqual(DEFAULT_SORT);
  });

  it("counts 480p and unknown releases as SD", () => {
    expect(
      tierCounts([
        release({ resolution: "480p" }),
        release({ resolution: "unknown" }),
        release({ resolution: "720p" }),
      ]),
    ).toEqual({ "2160p": 0, "1080p": 0, "720p": 1, SD: 2 });
  });
});
