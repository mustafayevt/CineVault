import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { titleLinks } from "./feedView";
import { ACTIONS_COLUMN_WIDTH, SEND_COLUMN_WIDTH } from "./ReleaseTable";
import { qualityRank } from "./releaseSort";
import { TorrentRequestError, setTorrentBackend } from "./torrentBackend";
import type { FeedStart } from "./torrentBackend";
import { backendWith, release, renderTorrents, spiedBackend } from "./torrentHarness";
import { applyFeedEvent, applyFeedStart, forgetLatestFeed } from "./useLatestFeed";
import type { FeedData } from "./useLatestFeed";

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  forgetLatestFeed();
  useToastStore.setState({ toasts: [], history: [] });
  useNotificationStore.setState({ items: [] });
});
afterEach(() => {
  restoreLayout();
  setTorrentBackend(null);
});

const feedGrid = () => screen.findByRole("grid", { name: "Latest releases" });
const feedRows = async () => (await within(await feedGrid()).findAllByRole("row")).slice(1);
const feedNames = async () =>
  (await feedRows()).map((row) => within(row).getAllByRole("gridcell")[1]?.textContent ?? "");
const rowNamed = (text: string) =>
  waitFor(
    async () => {
      const rows = await feedRows();
      const found = rows.find((row) => row.textContent?.includes(text));
      if (found === undefined) throw new Error(`no row ${text}`);
      return found;
    },
    { timeout: 3000 },
  );

describe("the Latest releases feed", () => {
  it("lists the newest releases of the sources as they answer, in the release table", async () => {
    const backend = spiedBackend();
    await renderTorrents("/torrents", backend);
    expect(await screen.findByTestId("latest-feed")).toBeVisible();
    await waitFor(async () => expect((await feedNames()).length).toBeGreaterThan(4));
    const first = backend.feedCalls.requests[0];
    expect(first).toMatchObject({ kind: "all", more: false, refresh: false });
    // The release table of the search: its columns and the newest first.
    expect(screen.getByRole("columnheader", { name: "Age" })).toHaveAttribute(
      "aria-sort",
      "ascending",
    );
    const names = await feedNames();
    expect(names.some((name) => name.includes("Severance.S02E10"))).toBe(true);
    expect(names.some((name) => name.includes("Blade Runner 2049 (2017) 2160p"))).toBe(true);
    // The search field and the responsibility line stay.
    expect(screen.getByRole("combobox", { name: "Search torrents" })).toBeVisible();
    expect(screen.getByTestId("torrent-responsibility")).toHaveTextContent(
      "You are responsible for what you download and share.",
    );
    // An indexer that cannot list without words is named quietly; Torrentio is not.
    const status = await screen.findByTestId("feed-status");
    await waitFor(() => expect(status).toHaveTextContent("1337x can't list new releases"));
    expect(status).not.toHaveTextContent("Torrentio");
  });

  it("asks again when the notice was accepted a moment before the setting was saved", async () => {
    const backend = spiedBackend();
    let refused = false;
    const latest = backend.latest;
    backend.latest = (request, onEvent) => {
      if (!refused) {
        refused = true;
        return Promise.reject(new TorrentRequestError("notice_not_accepted", "Accept first"));
      }
      return latest(request, onEvent);
    };
    await renderTorrents("/torrents", backend);
    await rowNamed("Oppenheimer");
    expect(refused).toBe(true);
  });

  it("marks titles the library holds, and a better copy, without asking TMDB", async () => {
    const backend = spiedBackend();
    await renderTorrents("/torrents", backend);
    const better = await rowNamed("Blade Runner 2049 (2017) 2160p");
    expect(better).toHaveTextContent("Better than your copy");
    const same = await rowNamed("Blade.Runner.2049.2017.1080p");
    expect(same).toHaveTextContent("In your library");
    expect(backend.feedCalls.lookups).toHaveLength(0);
  });

  it("switches between all, movies and TV", async () => {
    const backend = spiedBackend();
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    await rowNamed("Severance.S02E10");
    const kinds = screen.getByRole("group", { name: "Show" });
    await user.click(within(kinds).getByRole("button", { name: "Movies" }));
    await waitFor(() =>
      expect(backend.feedCalls.requests.some((request) => request.kind === "movies")).toBe(true),
    );
    await rowNamed("Past Lives (2023)");
    expect((await feedNames()).some((name) => name.includes("Severance"))).toBe(false);
    await user.click(within(kinds).getByRole("button", { name: "TV" }));
    await rowNamed("Severance.S02E10");
    expect((await feedNames()).some((name) => name.includes("Past Lives"))).toBe(false);
  });

  it("hides camera copies until asked, and sorts by the drop-down", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents", spiedBackend());
    await rowNamed("Oppenheimer");
    expect((await feedNames()).some((name) => name.includes("HDCAM"))).toBe(false);
    const hide = screen.getByRole("button", { name: "Hide CAM and TS" });
    expect(hide).toHaveAttribute("aria-pressed", "true");
    await user.click(hide);
    await rowNamed("Dune.Part.Two.2024.HDCAM");

    await user.click(screen.getByRole("combobox", { name: "Sort by" }));
    await user.click(await screen.findByRole("menuitemradio", { name: "Seeders" }));
    await waitFor(() =>
      expect(screen.getByRole("columnheader", { name: "Seeders" })).toHaveAttribute(
        "aria-sort",
        "descending",
      ),
    );
    const names = await feedNames();
    expect(names[0]).toContain("Severance.S02E10");
  });

  it("reads the next page near the end, never twice the same row, and says which sources are done", async () => {
    const backend = spiedBackend();
    await renderTorrents("/torrents", backend);
    // Few rows fill less than the screen, so the next pages are read until the sources end.
    await rowNamed("Poor Things (2023)");
    await rowNamed("Heat (1995) 2160p");
    expect(backend.feedCalls.requests.some((request) => request.more)).toBe(true);
    const ids = (await feedRows()).map((row) => row.textContent);
    expect(new Set(ids).size).toBe(ids.length);
    const status = await screen.findByTestId("feed-status");
    await waitFor(() => expect(status).toHaveTextContent(/No more from .*YTS/));
    await waitFor(() => expect(status).toHaveTextContent("That is everything"));
  });

  it("keeps the rows when a title is searched and the field is cleared again", async () => {
    const backend = spiedBackend({ failing: [] });
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    await rowNamed("Oppenheimer");
    const field = screen.getByRole("combobox", { name: "Search torrents" });
    await user.type(field, "blade runner");
    await user.click(await screen.findByRole("option", { name: /Blade Runner 2049 \(2017\)/ }));
    await screen.findByRole("grid", { name: "Releases" });
    expect(screen.queryByTestId("latest-feed")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    await rowNamed("Oppenheimer");
  });
});

describe("the details panel", () => {
  it("opens from the Details button and looks the title up once", async () => {
    const backend = spiedBackend();
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    const row = await rowNamed("Past Lives (2023)");
    expect(backend.feedCalls.lookups).toHaveLength(0);
    await user.click(within(row).getByRole("button", { name: "Details" }));
    const panel = await screen.findByRole("complementary", { name: "Release details" });
    expect(await within(panel).findByRole("heading", { name: "Past Lives (2023)" })).toBeVisible();
    expect(backend.feedCalls.lookups).toEqual([
      { kind: "movie", title: "Past Lives", year: 2023, imdbId: "tt13238346" },
    ]);
    // Links to the title's pages, IMDb from TMDB's IDs; Letterboxd for a movie.
    for (const name of ["TMDB", "IMDb", "Letterboxd", "Trakt"]) {
      expect(await within(panel).findByRole("link", { name: new RegExp(name) })).toBeVisible();
    }
    expect(within(panel).queryByText("This may not be the right title.")).toBeNull();
    // The hand-off stays: Send in the panel sends the magnet.
    await user.click(within(panel).getByRole("button", { name: "Send to torrent app" }));
    await waitFor(() => expect(backend.openMagnet).toHaveBeenCalledTimes(1));

    // Closed with Esc, opened again: no second lookup.
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("complementary", { name: "Release details" })).toBeNull(),
    );
    await user.click(
      within(await rowNamed("Past Lives (2023)")).getByRole("button", { name: "Details" }),
    );
    await screen.findByRole("complementary", { name: "Release details" });
    expect(backend.feedCalls.lookups).toHaveLength(1);
  });

  it("opens on Enter and double-click, never sending by accident", async () => {
    const backend = spiedBackend();
    const user = userEvent.setup();
    await renderTorrents("/torrents", backend);
    const row = await rowNamed("Oppenheimer");
    await user.dblClick(within(row).getAllByRole("gridcell")[1] as HTMLElement);
    expect(await screen.findByRole("complementary", { name: "Release details" })).toBeVisible();
    expect(backend.openMagnet).not.toHaveBeenCalled();
  });

  it("says when the match is weak and offers another title", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents", spiedBackend());
    const row = await rowNamed("Blade II (2002)");
    await user.click(within(row).getByRole("button", { name: "Details" }));
    const panel = await screen.findByRole("complementary", { name: "Release details" });
    expect(await within(panel).findByText("This may not be the right title.")).toBeVisible();
    await user.click(within(panel).getByRole("button", { name: "Pick another" }));
    expect(within(panel).getByRole("textbox", { name: "Find the right title" })).toHaveValue(
      "Blade II 2002",
    );
  });

  it("says when TMDB has nothing and lists the title's other releases", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents", spiedBackend());
    const row = await rowNamed("Harbour Lights (2026)");
    await user.click(within(row).getByRole("button", { name: "Details" }));
    const panel = await screen.findByRole("complementary", { name: "Release details" });
    expect(
      await within(panel).findByText("TMDB has no title that fits this release."),
    ).toBeVisible();
    expect(within(panel).queryByRole("link", { name: /TMDB/ })).toBeNull();
  });

  it("shows what the release name says when there is no TMDB key", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents", spiedBackend({ feedNoKey: true }));
    const row = await rowNamed("Severance.S02E10");
    await user.click(within(row).getByRole("button", { name: "Details" }));
    const panel = await screen.findByRole("complementary", { name: "Release details" });
    expect(await within(panel).findByText(/Add a TMDB key in Settings/)).toBeVisible();
    expect(within(panel).getByRole("heading", { name: "Severance" })).toBeVisible();
    expect(within(panel).getByText("TV show · S02E10")).toBeVisible();
    // The other release of the show in the list.
    expect(
      within(panel).getByText("Severance.S02E09.2160p.ATVP.WEB-DL.DV.HDR.H.265-GRP"),
    ).toBeVisible();
  });
});

describe("the release table's actions", () => {
  it("keeps Send in a column wide enough for it, with its full name", async () => {
    await renderTorrents(
      "/torrents?title=movie:1",
      backendWith([release({ id: "a1", name: "A.2160p" })]),
    );
    const grid = await screen.findByRole("grid", { name: "Releases" });
    const header = within(grid).getByRole("columnheader", { name: "Actions" });
    expect(header.getAttribute("style") ?? "").toContain(`${SEND_COLUMN_WIDTH}px`);
    const cell = (await within(grid).findAllByTestId("release-actions"))[0] as HTMLElement;
    expect(within(cell).getByRole("button", { name: "Send to torrent app" })).toHaveTextContent(
      "Send",
    );
    expect(within(cell).queryByRole("button", { name: "Details" })).toBeNull();
  });

  it("gives the feed a Details button next to Send in a wider column", async () => {
    await renderTorrents("/torrents", spiedBackend());
    const row = await rowNamed("Oppenheimer");
    const header = screen.getByRole("columnheader", { name: "Actions" });
    expect(header.getAttribute("style") ?? "").toContain(`${ACTIONS_COLUMN_WIDTH}px`);
    expect(within(row).getByRole("button", { name: "Details" })).toBeVisible();
    expect(within(row).getByRole("button", { name: "Send to torrent app" })).toBeVisible();
  });
});

describe("the feed's pieces", () => {
  const empty: FeedData = {
    generation: null,
    rows: new Map(),
    sources: [],
    running: 1,
    started: true,
    paused: false,
  };

  it("folds the start reply in without undoing what the stream already said", () => {
    const start: FeedStart = {
      generation: "4",
      sources: [
        { id: "yts", name: "YTS", kind: "built-in", state: "asked" },
        { id: "eztv", name: "EZTV", kind: "built-in", state: "asked" },
      ],
      rows: [],
    };
    let data = applyFeedStart(empty, start, new Set());
    data = applyFeedEvent(data, {
      kind: "results",
      provider: "yts",
      items: [{ type: "sourceState", state: "done" }],
    });
    // A late start reply keeps YTS done.
    data = applyFeedStart(data, start, new Set(["yts"]));
    expect(data.sources.map((source) => source.state)).toEqual(["done", "asked"]);
    expect(data.generation).toBe("4");
  });

  it("replaces a row sent again and drops the rows it absorbed", () => {
    const row = (id: string, absorbs: string[] = []) => ({
      release: release({ id, absorbs }),
      kind: "movie",
      title: "A",
      year: 2026,
      season: null,
      episode: null,
      imdbId: null,
      titleKey: "movie|a|2026",
      inLibrary: null,
    });
    let data = applyFeedEvent(empty, {
      kind: "partial",
      provider: "yts",
      items: [
        { type: "row", row: row("x") },
        { type: "row", row: row("y") },
      ],
    });
    data = applyFeedEvent(data, {
      kind: "results",
      provider: "knaben",
      items: [{ type: "row", row: row("z", ["x"]) }],
    });
    expect([...data.rows.keys()]).toEqual(["y", "z"]);
  });

  it("ranks quality by resolution, HDR and source, a camera copy last", () => {
    const rank = (resolution: string, hdr: string | null, source: string | null, cam = false) =>
      qualityRank({ resolution, hdr, source, flags: cam ? [{ kind: "cam", text: "" }] : [] });
    expect(rank("2160p", "DV", "Remux")).toBeGreaterThan(rank("2160p", null, "Remux"));
    expect(rank("2160p", null, "WEB-DL")).toBeGreaterThan(rank("1080p", "DV", "Remux"));
    expect(rank("1080p", null, "BluRay")).toBeGreaterThan(rank("1080p", null, "WEBRip"));
    expect(rank("2160p", null, null, true)).toBe(0);
  });

  it("builds the title links, with no Letterboxd for a show", () => {
    expect(titleLinks("movie", 603, "tt0133093").map((link) => link.url)).toEqual([
      "https://www.themoviedb.org/movie/603",
      "https://www.imdb.com/title/tt0133093/",
      "https://letterboxd.com/tmdb/603/",
      "https://trakt.tv/search/tmdb/603?id_type=movie",
    ]);
    expect(titleLinks("show", 1399, null).map((link) => link.label)).toEqual(["TMDB", "Trakt"]);
  });
});
