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
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(backend.titleStarts).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Got it" }));
    expect(await screen.findByRole("searchbox", { name: "Search torrents" })).toBeVisible();
  });

  it("finds the best match after the typing pause, names it and lists its releases", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    const field = await screen.findByRole("searchbox", { name: "Search torrents" });
    expect(screen.getByText("Type a movie or show to see its releases.")).toBeVisible();
    await user.type(field, "blade runner");
    // No search while the typing goes on (the pause is 400 ms).
    expect(backend.titleStarts).toHaveLength(0);
    await waitFor(() => expect(backend.titleStarts.length).toBeGreaterThan(0), { timeout: 3000 });
    expect(backend.titleStarts.at(-1)?.query).toBe("blade runner");
    expect((await screen.findAllByText("Blade Runner 2049 (2017)")).length).toBeGreaterThan(0);
    await waitFor(() => expect(backend.releaseStarts.length).toBeGreaterThan(0), { timeout: 3000 });
    expect((await dataRows()).length).toBeGreaterThan(3);
    expect(await screen.findByText(/^\d+ results? · \d+ sources?$/)).toBeVisible();
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
    await user.type(await screen.findByRole("searchbox"), "zzzzqqqq");
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
