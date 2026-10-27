import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { settleSettingSaves, useSettingsStore } from "../settings/settingsStore";
import { setTorrentBackend } from "./torrentBackend";
import { renderTorrents, spiedBackend } from "./torrentHarness";

afterEach(() => setTorrentBackend(null));

describe("the responsibility notice of Torrent search", () => {
  it("is the first thing on the first visit, and no source is contacted before it", async () => {
    const backend = spiedBackend();
    const sourcesStatus = vi.spyOn(backend, "sourcesStatus");
    await renderTorrents("/torrents?title=movie:1", backend, false);
    const notice = await screen.findByTestId("torrent-notice");
    expect(notice).toHaveTextContent("Torrent search finds releases on public sites.");
    expect(notice).toHaveTextContent("You are responsible for what you download and share.");
    expect(screen.queryByRole("searchbox", { name: "Search torrents" })).toBeNull();
    expect(backend.titleStarts).toEqual([]);
    expect(backend.releaseStarts).toEqual([]);
    expect(sourcesStatus).not.toHaveBeenCalled();
  });

  it("goes away with Got it, which remembers the answer, and the search follows", async () => {
    const user = userEvent.setup();
    const backend = spiedBackend();
    await renderTorrents("/torrents", backend, false);
    await user.click(await screen.findByRole("button", { name: "Got it" }));
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["torrents.notice-accepted"]).toBe(true);
    await waitFor(() => expect(screen.queryByTestId("torrent-notice")).toBeNull());
    expect(await screen.findByRole("searchbox", { name: "Search torrents" })).toBeVisible();
  });

  it("is not shown again once accepted", async () => {
    await renderTorrents("/torrents");
    expect(screen.queryByTestId("torrent-notice")).toBeNull();
    expect(await screen.findByRole("searchbox", { name: "Search torrents" })).toBeVisible();
  });
});
