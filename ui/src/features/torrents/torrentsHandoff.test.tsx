import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { useNotificationStore } from "../notifications/notificationStore";
import { useToastStore } from "../toasts/toastStore";
import { TorrentRequestError, setTorrentBackend } from "./torrentBackend";
import { backendWith, release, renderTorrents } from "./torrentHarness";

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
describe("the copy you own", () => {
  it("shows the Yours line and a Same, Better or Worse hint on each release", async () => {
    await renderTorrents(
      "/torrents?title=movie:1",
      backendWith([
        release({ id: "a", name: "A", upgrade: "better" }),
        release({ id: "b", name: "B", upgrade: "worse" }),
        release({ id: "c", name: "C", upgrade: "same" }),
        release({ id: "d", name: "D", upgrade: null }),
      ]),
    );
    const rows = await dataRows();
    await waitFor(() => expect(screen.getByText(/Yours:/)).toBeVisible());
    expect(screen.getByText(/1080p WEB-DL|1080p/, { selector: "span.truncate" })).toHaveTextContent(
      /Yours: Blade Runner 2049 \(2017\) · 1080p · 5\.6 GB/,
    );
    expect(within(rows[0] as HTMLElement).getByText("Better")).toBeVisible();
    expect(within(rows[1] as HTMLElement).getByText("Worse")).toBeVisible();
    expect(within(rows[2] as HTMLElement).getByText("Same as yours")).toBeVisible();
    expect(within(rows[3] as HTMLElement).queryByText(/Better|Worse|Same as yours/)).toBeNull();
  });
});

describe("handing a release to the torrent app", () => {
  it("sends the magnet link on double-click and says so", async () => {
    const user = userEvent.setup();
    const backend = backendWith([release({ id: "a", name: "A", magnet: "magnet:?xt=urn:btih:a" })]);
    await renderTorrents("/torrents?title=movie:1", backend);
    const [row] = await dataRows();
    await user.dblClick(row as HTMLElement);
    await waitFor(() => expect(backend.openMagnet).toHaveBeenCalledWith("magnet:?xt=urn:btih:a"));
    expect(useToastStore.getState().toasts.map((toast) => toast.message)).toContain(
      "Sent to your torrent app",
    );
  });

  it("sends with Enter on the chosen row and from the row's button", async () => {
    const user = userEvent.setup();
    const backend = backendWith([
      release({ id: "a", name: "A", magnet: "magnet:?xt=urn:btih:a" }),
      release({ id: "b", name: "B", magnet: "magnet:?xt=urn:btih:b", seeders: 1 }),
    ]);
    await renderTorrents("/torrents?title=movie:1", backend);
    const [first, second] = await dataRows();
    await user.click(first as HTMLElement);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(backend.openMagnet).toHaveBeenCalledWith("magnet:?xt=urn:btih:a"));
    await user.click(
      within(second as HTMLElement).getByRole("button", { name: "Send to torrent app" }),
    );
    await waitFor(() => expect(backend.openMagnet).toHaveBeenCalledWith("magnet:?xt=urn:btih:b"));
  });

  it("says so, and offers to copy the link, when no torrent app is set up", async () => {
    const user = userEvent.setup();
    const backend = backendWith([release({ id: "a", name: "A" })]);
    backend.openMagnet = () => Promise.reject(new TorrentRequestError("no_handler", "none"));
    await renderTorrents("/torrents?title=movie:1", backend);
    const [row] = await dataRows();
    await user.dblClick(row as HTMLElement);
    await waitFor(() =>
      expect(
        useToastStore.getState().toasts.some((toast) => /No torrent app/.test(toast.message)),
      ).toBe(true),
    );
    const toast = useToastStore.getState().toasts.find((entry) => entry.action !== undefined);
    expect(toast?.action?.label).toBe("Copy magnet link");
  });

  it("copies the magnet link from the right-click menu", async () => {
    const user = userEvent.setup();
    const backend = backendWith([release({ id: "a", name: "A", magnet: "magnet:?xt=urn:btih:a" })]);
    await renderTorrents("/torrents?title=movie:1", backend);
    const [row] = await dataRows();
    await act(async () => {
      row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 5, clientY: 5 }));
    });
    const menu = await screen.findByRole("menu");
    expect(within(menu).getByRole("menuitem", { name: /Send to torrent app/ })).toBeVisible();
    await user.click(within(menu).getByRole("menuitem", { name: "Copy magnet link" }));
    await waitFor(async () =>
      expect(await navigator.clipboard.readText()).toBe("magnet:?xt=urn:btih:a"),
    );
  });
});
