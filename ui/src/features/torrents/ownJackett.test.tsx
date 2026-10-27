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
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setProvidersBackend, unavailableBackend } from "../providers/providersBackend";
import { useSettingsLoader } from "../settings/appearance";
import { SettingsPage } from "../settings/SettingsPage";
import { createMemoryBackend, setSettingsBackend } from "../settings/settingsBackend";
import { useSettingsStore } from "../settings/settingsStore";
import { setTorrentBackend } from "./torrentBackend";
import { spiedBackend } from "./torrentHarness";
import type { SpiedBackend } from "./torrentHarness";
import { forgetOwnJackett } from "./useOwnJackett";

const FEED = "http://127.0.0.1:9117/api/v2.0/indexers/all/results/torznab/api";

function Page() {
  useSettingsLoader();
  return <SettingsPage />;
}

async function openSettings(torrents: SpiedBackend = spiedBackend()) {
  setSettingsBackend(createMemoryBackend({}));
  setTorrentBackend(torrents);
  useSettingsStore.setState({ values: {}, status: "idle" });
  const rootRoute = createRootRoute();
  const router = createRouter({
    routeTree: rootRoute.addChildren([
      createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: Page }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/settings?section=torrents"] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByTestId("indexer-card");
  await waitFor(() => expect(useSettingsStore.getState().status).toBe("ready"));
  return torrents;
}

beforeEach(() => {
  forgetOwnJackett();
  setProvidersBackend(unavailableBackend);
});

afterEach(() => {
  setTorrentBackend(null);
  setSettingsBackend(null);
});

describe("use your Jackett", () => {
  it("is offered when one answers, takes the pasted key and uses its all-indexers feed", async () => {
    const user = userEvent.setup();
    const torrents = await openSettings();
    const card = await screen.findByTestId("own-jackett");
    expect(card).toHaveTextContent("Jackett answers at http://127.0.0.1:9117");
    await user.click(within(card).getByRole("button", { name: "Use your Jackett" }));
    const field = within(card).getByLabelText("Jackett API key");
    expect(card).toHaveTextContent("copy the API key at its top right");
    const useIt = within(card).getByRole("button", { name: "Use it" });
    expect(useIt).toBeDisabled();
    await user.type(field, "  pasted-key ");
    await user.click(useIt);
    await waitFor(() => expect(card).toHaveAttribute("data-in-use", "true"));
    expect(torrents.calls.torznab).toEqual([
      { url: FEED, apiKey: "pasted-key" },
      { url: FEED, apiKey: "pasted-key" },
      // The card in use reads the indexer count with the saved key.
      { url: FEED, apiKey: null },
    ]);
    expect(useSettingsStore.getState().values["torrents.torznab.url"]).toBe(FEED);
    expect(card).toHaveTextContent("Searches ask your Jackett too: 12 indexers.");
    expect(within(card).getByText("In use")).toBeInTheDocument();
  });

  it("offers to switch CineVault's own indexer service off while it is used", async () => {
    const user = userEvent.setup();
    const torrents = spiedBackend();
    await torrents.setTorznab(FEED, "saved-key");
    await openSettings(torrents);
    const card = await screen.findByTestId("own-jackett");
    await waitFor(() => expect(card).toHaveAttribute("data-in-use", "true"));
    const managed = within(card).getByTestId("own-jackett-managed");
    await user.click(
      within(managed).getByRole("button", { name: "Switch the indexer service off" }),
    );
    await waitFor(() => expect(torrents.setSource).toHaveBeenCalledWith("indexer-service", false));
  });

  it("opens Jackett's dashboard for the key and says when the key is wrong", async () => {
    const user = userEvent.setup();
    const torrents = spiedBackend();
    const rejected = vi.fn(() =>
      Promise.resolve({ ok: false, message: "The key was rejected", caps: null }),
    );
    await openSettings({ ...torrents, testTorznab: rejected });
    const card = await screen.findByTestId("own-jackett");
    await user.click(within(card).getByRole("button", { name: "Use your Jackett" }));
    await user.click(within(card).getByRole("button", { name: "Open Jackett" }));
    expect(torrents.openPage).toHaveBeenCalledWith("http://127.0.0.1:9117/UI/Dashboard");
    await user.type(within(card).getByLabelText("Jackett API key"), "wrong");
    await user.click(within(card).getByRole("button", { name: "Use it" }));
    await waitFor(() =>
      expect(within(card).getByRole("status")).toHaveTextContent("The key was rejected"),
    );
    expect(torrents.calls.torznab).toEqual([]);
    expect(useSettingsStore.getState().values["torrents.torznab.url"]).toBeUndefined();
  });

  it("is not there when no Jackett of the person's own answers", async () => {
    await openSettings(spiedBackend({ ownJackett: false }));
    await screen.findByTestId("torznab-card");
    expect(screen.queryByTestId("own-jackett")).toBeNull();
  });

  it("has no accessibility violations", async () => {
    const user = userEvent.setup();
    await openSettings();
    const card = await screen.findByTestId("own-jackett");
    await user.click(within(card).getByRole("button", { name: "Use your Jackett" }));
    const result = await axe.run(card);
    expect(result.violations.map((v) => `${v.id}: ${v.nodes[0]?.html.slice(0, 80)}`)).toEqual([]);
  });
});
