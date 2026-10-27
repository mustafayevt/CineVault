import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { setProvidersBackend, unavailableBackend } from "../providers/providersBackend";
import { useSettingsLoader } from "../settings/appearance";
import { SettingsPage } from "../settings/SettingsPage";
import { createMemoryBackend, setSettingsBackend } from "../settings/settingsBackend";
import { settleSettingSaves, useSettingsStore } from "../settings/settingsStore";
import { setTorrentBackend } from "./torrentBackend";
import { spiedBackend } from "./torrentHarness";
import type { SpiedBackend } from "./torrentHarness";
import { checkedText, healthTag } from "./sourceHealth";

function Page() {
  useSettingsLoader();
  return <SettingsPage />;
}

async function renderSettings(
  values: Record<string, unknown>,
  path: string,
  torrents: SpiedBackend = spiedBackend(),
) {
  const backend = createMemoryBackend(values);
  setSettingsBackend(backend);
  setTorrentBackend(torrents);
  useSettingsStore.setState({ values: {}, status: "idle" });
  const rootRoute = createRootRoute();
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: Page }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByRole("heading", { level: 1, name: "Settings" });
  await waitFor(() => expect(useSettingsStore.getState().status).toBe("ready"));
  return { backend, torrents };
}

beforeEach(() => {
  setProvidersBackend(unavailableBackend);
});

afterEach(() => {
  setSettingsBackend(null);
  setProvidersBackend(null);
  setTorrentBackend(null);
});

function rowOf(list: HTMLElement, id: string): HTMLElement {
  const row = list.querySelector<HTMLElement>(`[data-source="${id}"]`);
  if (row === null) throw new Error(`no row for ${id}`);
  return row;
}

const SECTION = "/settings?section=torrents";
const ON = {};

describe("Settings, Torrent sources", () => {
  it("lists a row per source with its words for health and its last check", async () => {
    await renderSettings(ON, SECTION);
    const list = await screen.findByRole("list", { name: "Sites" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((row) => row.getAttribute("data-source"))).toEqual([
      "torrentio",
      "yts",
      "eztv",
      "apibay",
      "knaben",
      "nyaa",
    ]);
    const knaben = rowOf(list, "knaben");
    const first = rowOf(list, "torrentio");
    // Health is a word, never colour alone.
    expect(within(knaben).getByText("Error")).toBeVisible();
    expect(within(knaben).getByText("Rate limited")).toBeVisible();
    expect(within(first).getByText("Working")).toBeVisible();
    expect(within(first).getByText(/^Checked /)).toBeVisible();
    for (const row of rows) {
      expect(within(row).getByRole("switch")).toHaveAccessibleName();
    }
  });

  it("calls the command when a switch is used and shows the answer", async () => {
    const user = userEvent.setup();
    const { torrents } = await renderSettings(ON, SECTION);
    const row = rowOf(await screen.findByRole("list", { name: "Sites" }), "yts");
    await user.click(within(row).getByRole("switch", { name: "YTS" }));
    await waitFor(() => expect(torrents.calls.switched).toEqual([{ id: "yts", on: false }]));
    await waitFor(() => expect(row.querySelector('[data-status="off"]')).not.toBeNull());
    expect(within(row).getByRole("switch", { name: "YTS" })).not.toBeChecked();
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["torrents.source.yts"]).toBe(false);
  });

  it("reads the sources again on Check sources now", async () => {
    const user = userEvent.setup();
    const torrents = spiedBackend();
    let reads = 0;
    const read = torrents.sourcesStatus;
    torrents.sourcesStatus = () => {
      reads += 1;
      return read();
    };
    await renderSettings(ON, SECTION, torrents);
    await screen.findByRole("list", { name: "Sites" });
    const before = reads;
    await user.click(screen.getByRole("button", { name: "Check sources now" }));
    await waitFor(() => expect(reads).toBeGreaterThan(before));
  });

  it("lists the indexer service and the own Torznab instance below the sites", async () => {
    await renderSettings(ON, SECTION);
    await screen.findByRole("list", { name: "Sites" });
    expect(screen.getByTestId("indexer-card")).toBeVisible();
    expect(document.getElementById("torrents.torznab.url")).not.toBeNull();
  });
});

describe("the words", () => {
  it("name every health, with a tag that is not only colour", () => {
    expect(healthTag("working")).toEqual({ status: "done", label: "Working" });
    expect(healthTag("needsSolver")).toEqual({
      status: "needs-key",
      label: "Needs a challenge solver",
    });
    expect(healthTag("unreachable").label).toBe("Unreachable");
    expect(healthTag("error").label).toBe("Error");
    expect(healthTag("checking").status).toBe("in-progress");
    expect(healthTag("off").status).toBe("off");
    expect(healthTag("untested").label).toBe("Not checked yet");
  });

  it("say when a source was last asked", () => {
    const now = Date.UTC(2026, 9, 26, 12, 0, 0);
    expect(checkedText(null, now)).toBe("Not checked yet");
    expect(checkedText(now - 5_000, now)).toBe("Checked just now");
    expect(checkedText(now - 5 * 60_000, now)).toBe("Checked 5 min ago");
    expect(checkedText(now - 2 * 3_600_000, now)).toBe("Checked 2 h ago");
    expect(checkedText(now - 3 * 86_400_000, now)).toBe("Checked 3 d ago");
  });
});
