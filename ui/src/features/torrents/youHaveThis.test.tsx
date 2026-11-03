import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { restoreLayout, stubLayout } from "../../test/layout";
import { useToastStore } from "../toasts/toastStore";
import { setTorrentBackend } from "./torrentBackend";
import { backendWith, release, renderTorrents, spiedBackend } from "./torrentHarness";

beforeEach(() => {
  stubLayout();
  window.localStorage.clear();
  useToastStore.setState({ toasts: [], history: [] });
});
afterEach(() => {
  restoreLayout();
  setTorrentBackend(null);
});

const grid = () => screen.findByRole("grid", { name: "Releases" });
const dataRows = async () => (await within(await grid()).findAllByRole("row")).slice(1);
const libraryChip = (name: string) =>
  within(screen.getByRole("group", { name: "Your library" })).getByRole("button", {
    name: new RegExp(`^${name}`),
  });

describe("You have this", () => {
  it("lists every copy best first and says releases are compared with the first", async () => {
    await renderTorrents("/torrents?title=movie:3", backendWith([release({ upgrade: "better" })]));
    const card = await screen.findByRole("region", { name: "You have this" });
    expect(card).toHaveTextContent("2 copies");
    const lines = within(card).getAllByRole("listitem");
    expect(lines[0]).toHaveTextContent("1080p · AVC · DTS-HD MA 5.1 · BluRay · Remastered");
    expect(lines[0]).toHaveTextContent("EN · FR");
    expect(lines[1]).toHaveTextContent("720p");
    expect(card).toHaveTextContent("Releases are compared with the first.");
  });

  it("folds into one line with Search anyway and opens again", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents?title=movie:1", backendWith([release({ upgrade: "better" })]));
    const card = await screen.findByRole("region", { name: "You have this" });
    await user.click(within(card).getByRole("button", { name: "Search anyway" }));
    expect(screen.queryByRole("region", { name: "You have this" })).toBeNull();
    const line = screen.getByTestId("you-have-this");
    expect(line).toHaveAttribute("data-folded", "true");
    expect(line).toHaveTextContent(
      "You have this: Blade Runner 2049 (2017) · 1080p · AVC · 5.6 GB",
    );
    await user.click(screen.getByRole("button", { name: "Show what you have" }));
    expect(screen.getByRole("region", { name: "You have this" })).toBeVisible();
  });

  it("has no card for a title the library does not hold", async () => {
    await renderTorrents("/torrents?title=movie:2", backendWith([release()]));
    await dataRows();
    expect(screen.queryByTestId("you-have-this")).toBeNull();
    expect(screen.queryByRole("group", { name: "Your library" })).toBeNull();
  });
});

describe("Compare with yours", () => {
  it("opens from the tag with both sides row by row and the verdict, and sends nothing", async () => {
    const user = userEvent.setup();
    const backend = backendWith([
      release({
        id: "a",
        name: "Blade.Runner.2049.2017.2160p.UHD.BluRay.REMUX.DV.TrueHD.Atmos-GRP",
        resolution: "2160p",
        source: "Remux",
        codec: "HEVC",
        hdr: "DV",
        audio: "TrueHD Atmos 7.1",
        size: 14_000_000_000,
        upgrade: "better",
      }),
    ]);
    await renderTorrents("/torrents?title=movie:1", backend);
    const [row] = await dataRows();
    await user.click(within(row as HTMLElement).getByTestId("compare-tag"));
    const popover = await screen.findByRole("dialog", { name: "Compare with yours" });
    const block = within(popover).getByTestId("compare-block");
    const resolution = within(block).getByRole("row", { name: /Resolution/ });
    expect(resolution).toHaveTextContent("1080p");
    expect(resolution).toHaveTextContent("2160p");
    expect(resolution).toHaveAttribute("data-differs", "true");
    expect(within(block).getByRole("row", { name: /HDR/ })).toHaveTextContent("SDRDolby Vision");
    expect(within(block).getByRole("row", { name: /Codec/ })).toHaveAttribute("data-differs");
    expect(within(block).getByRole("row", { name: /Edition/ })).not.toHaveAttribute("data-differs");
    expect(within(popover).getByTestId("compare-verdict")).toHaveTextContent(
      "Better picture, better audio, 2.5× the size",
    );
    expect(backend.openMagnet).not.toHaveBeenCalled();
  });

  it("opens from the right-click menu too", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents?title=movie:1", backendWith([release({ upgrade: "worse" })]));
    const [row] = await dataRows();
    await act(async () => {
      row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 5, clientY: 5 }));
    });
    const item = await screen.findByRole("menuitem", { name: /Compare with yours/ });
    await user.click(item);
    const popover = await screen.findByRole("dialog", { name: "Compare with yours" });
    expect(within(popover).getByTestId("compare-verdict")).toHaveTextContent(/^Worse picture/);
  });
});

describe("Only better than mine", () => {
  it("hides the same and worse releases and goes into the address", async () => {
    const user = userEvent.setup();
    const { router } = await renderTorrents(
      "/torrents?title=movie:1",
      backendWith([
        release({ id: "a", name: "A", upgrade: "better" }),
        release({ id: "b", name: "B", upgrade: "worse" }),
        release({ id: "c", name: "C", upgrade: "same" }),
      ]),
    );
    expect(await dataRows()).toHaveLength(3);
    const chip = libraryChip("Only better than mine");
    expect(chip).toHaveTextContent("1");
    await user.click(chip);
    expect(chip).toHaveAttribute("aria-pressed", "true");
    expect(await dataRows()).toHaveLength(1);
    await waitFor(() => expect(router.state.location.search).toMatchObject({ only: "better" }));
  });

  it("is on when the address says so, and Look for a better copy turns it on", async () => {
    const user = userEvent.setup();
    const rows = [
      release({ id: "a", name: "A", upgrade: "better" }),
      release({ id: "b", name: "B", upgrade: "worse" }),
    ];
    const first = await renderTorrents("/torrents?title=movie:1&only=better", backendWith(rows));
    expect(await dataRows()).toHaveLength(1);
    expect(libraryChip("Only better than mine")).toHaveAttribute("aria-pressed", "true");
    first.unmount();

    await renderTorrents("/torrents?title=movie:1", backendWith(rows));
    expect(await dataRows()).toHaveLength(2);
    await user.click(screen.getByRole("button", { name: "Look for a better copy" }));
    expect(await dataRows()).toHaveLength(1);
  });

  it("says when nothing is left and Clear filters shows everything again", async () => {
    const user = userEvent.setup();
    await renderTorrents(
      "/torrents?title=movie:1&only=better",
      backendWith([release({ id: "b", name: "B", upgrade: "worse" })]),
    );
    expect(await screen.findByText("No releases left by this filter.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(await dataRows()).toHaveLength(1);
  });
});

describe("a show against the episodes you have", () => {
  it("says what is held and missing, tags each release and filters the gaps", async () => {
    const user = userEvent.setup();
    await renderTorrents("/torrents?title=show:1&season=2", spiedBackend());
    const card = await screen.findByRole("region", { name: "You have this" });
    expect(card).toHaveTextContent("You have S01–S02 · missing S02E04–E10");
    const rows = await dataRows();
    const named = (text: RegExp) =>
      rows.find((row) => text.test(row.textContent ?? "")) as HTMLElement;
    expect(within(named(/Severance\.S02\.2160p/)).getByTestId("show-mark")).toHaveTextContent(
      "Fills a gap",
    );
    expect(within(named(/Severance\.S02E01\.1080p/)).getByTestId("show-mark")).toHaveTextContent(
      "In your library",
    );
    expect(within(named(/Severance\.S02E01\.2160p/)).getByTestId("show-mark")).toHaveTextContent(
      "Better",
    );
    await user.click(within(card).getByRole("button", { name: "Find missing episodes" }));
    const chip = libraryChip("Fills a gap");
    expect(chip).toHaveAttribute("aria-pressed", "true");
    const left = await dataRows();
    expect(left.length).toBeGreaterThan(0);
    for (const row of left) {
      expect(within(row).getByTestId("show-mark")).toHaveTextContent("Fills a gap");
    }
  });
});
