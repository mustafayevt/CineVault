import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { setProvidersBackend, unavailableBackend } from "../providers/providersBackend";
import { useSettingsLoader } from "../settings/appearance";
import { SettingsPage } from "../settings/SettingsPage";
import { createMemoryBackend, setSettingsBackend } from "../settings/settingsBackend";
import { useSettingsStore } from "../settings/settingsStore";
import { setTorrentBackend } from "./torrentBackend";
import { spiedBackend } from "./torrentHarness";

function Page() {
  useSettingsLoader();
  return <SettingsPage />;
}

async function openCard(state: string) {
  const torrents = spiedBackend({ indexerState: state });
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
  const element = await screen.findByTestId("indexer-card");
  await waitFor(() => expect(useSettingsStore.getState().status).toBe("ready"));
  return { torrents, element };
}

beforeEach(() => {
  setProvidersBackend(unavailableBackend);
});

afterEach(() => {
  setSettingsBackend(null);
  setProvidersBackend(null);
  setTorrentBackend(null);
});

const detail = () => screen.getByTestId("indexer-detail");

describe("the indexer service card", () => {
  it("says it is not downloaded and offers Download, which downloads and starts it", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("notInstalled");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "notInstalled"));
    expect(within(element).getByText("Not downloaded", { selector: "span" })).toBeVisible();
    expect(detail()).toHaveTextContent(/Not downloaded yet/);
    expect(within(element).queryByRole("button", { name: "Open indexer manager" })).toBeNull();
    await user.click(within(element).getByRole("button", { name: "Download" }));
    await waitFor(() => expect(torrents.calls.indexer).toContain("install"));
    await waitFor(() => expect(element).toHaveAttribute("data-state", "running"));
    expect(within(element).queryByRole("button", { name: "Download" })).toBeNull();
    expect(detail()).toHaveTextContent("Version 0.24.2903");
  });

  it("shows the version, what the last check found and every indexer with its health in words", async () => {
    const { element } = await openCard("running");
    const list = await within(element).findByTestId("indexer-list");
    await waitFor(() =>
      expect(detail()).toHaveTextContent("Version 0.24.2903 · 2 indexers work · 2 did not pass"),
    );
    const row = (id: string) => list.querySelector<HTMLElement>(`[data-indexer="${id}"]`);
    expect(row("limetorrents")).toHaveTextContent("Working");
    expect(row("1337x")).toHaveTextContent("Needs a challenge solver");
    expect(row("1337x")).toHaveTextContent("A browser challenge blocks it.");
    expect(row("torrentgalaxy")).toHaveTextContent("Unreachable");
    for (const item of within(list).getAllByRole("listitem")) {
      expect(within(item).getByRole("switch")).toHaveAccessibleName();
    }
  });

  it("switches one indexer off for good and back on", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("running");
    const list = await within(element).findByTestId("indexer-list");
    expect(within(list).getByRole("switch", { name: "LimeTorrents" })).toBeChecked();
    await user.click(within(list).getByRole("switch", { name: "LimeTorrents" }));
    await waitFor(() => expect(torrents.calls.indexer).toContain("set:limetorrents:false"));
    await waitFor(() =>
      expect(within(list).getByRole("switch", { name: "LimeTorrents" })).not.toBeChecked(),
    );
    expect(list.querySelector<HTMLElement>('[data-indexer="limetorrents"]')).toHaveTextContent(
      "Off",
    );
    await user.click(within(list).getByRole("switch", { name: "LimeTorrents" }));
    await waitFor(() => expect(torrents.calls.indexer).toContain("set:limetorrents:true"));
    await waitFor(() =>
      expect(within(list).getByRole("switch", { name: "LimeTorrents" })).toBeChecked(),
    );
  });

  it("offers Update only when a newer version was found, and installs it", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("running");
    expect(await within(element).findByTestId("indexer-latest")).toHaveTextContent(
      "Version 0.24.3000 is available",
    );
    await user.click(within(element).getByRole("button", { name: "Update indexer service" }));
    await waitFor(() => expect(torrents.calls.indexer).toContain("update"));
    await waitFor(() => expect(detail()).toHaveTextContent("Version 0.24.3000"));
    expect(within(element).queryByRole("button", { name: "Update indexer service" })).toBeNull();
    expect(within(element).queryByTestId("indexer-latest")).toBeNull();
  });

  it("has no Update while nothing newer was found", async () => {
    const { torrents, element } = await openCard("installed");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "installed"));
    expect(within(element).queryByRole("button", { name: "Update indexer service" })).toBeNull();
    expect(torrents.calls.indexer).toEqual([]);
  });

  it("opens the service page and keeps its password behind a Copy button", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("running");
    const openButton = await within(element).findByRole("button", {
      name: "Open indexer manager",
    });
    expect(screen.queryByTestId("indexer-password")).toBeNull();
    await user.click(openButton);
    await waitFor(() => expect(torrents.calls.indexer).toContain("open"));
    const group = await screen.findByTestId("indexer-password");
    // The password is never written on the page, only copied.
    expect(document.body.textContent).not.toContain("synthetic-admin-password");
    await user.click(within(group).getByRole("button", { name: "Copy the password" }));
    expect(await navigator.clipboard.readText()).toBe("synthetic-admin-password");
  });

  it("checks the indexers with Check sources now while the service runs", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("running");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "running"));
    await user.click(screen.getByRole("button", { name: "Check sources now" }));
    await waitFor(() => expect(torrents.calls.indexer).toContain("check"));
  });

  it("does not ask the service to check when it is not there", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("notInstalled");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "notInstalled"));
    await user.click(screen.getByRole("button", { name: "Check sources now" }));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    expect(torrents.calls.indexer).not.toContain("check");
  });

  it("says why it failed and offers to try again", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("failed");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "failed"));
    expect(detail()).toHaveTextContent("The port is taken");
    await user.click(within(element).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(torrents.calls.indexer).toContain("install"));
  });

  it("says the download is running while it downloads", async () => {
    const { element } = await openCard("downloading");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "downloading"));
    expect(detail()).toHaveTextContent(/download is running/);
    expect(within(element).queryByRole("button", { name: "Download" })).toBeNull();
  });

  it("turns the service off from its switch and says so", async () => {
    const user = userEvent.setup();
    const { torrents, element } = await openCard("running");
    await waitFor(() => expect(element).toHaveAttribute("data-state", "running"));
    await user.click(within(element).getByRole("switch", { name: "Indexer service" }));
    await waitFor(() =>
      expect(torrents.calls.switched).toContainEqual({ id: "indexer-service", on: false }),
    );
    await waitFor(() => expect(detail()).toHaveTextContent(/Off/));
    expect(within(element).queryByTestId("indexer-list")).toBeNull();
  });

  it("holds the challenge solver address", async () => {
    const { element } = await openCard("running");
    expect(within(element).getByText("Challenge solver address")).toBeInTheDocument();
  });

  it("says how to let the sites that ask for a browser check through", async () => {
    const user = userEvent.setup();
    const { element, torrents } = await openCard("running");
    const help = await within(element).findByTestId("solver-help");
    expect(help).toHaveTextContent("1 site asks for a browser check.");
    expect(help).toHaveTextContent("http://127.0.0.1:8191");
    await user.click(within(help).getByRole("button", { name: "How to install FlareSolverr" }));
    expect(torrents.openPage).toHaveBeenCalledWith(
      "https://github.com/FlareSolverr/FlareSolverr#installation",
    );
    // Gone once a solver address is set.
    act(() => {
      useSettingsStore
        .getState()
        .set("torrents.indexer-service.solver-url", "http://127.0.0.1:8191");
    });
    await waitFor(() => expect(within(element).queryByTestId("solver-help")).toBeNull());
  });

  it("has no accessibility violations", async () => {
    const { element } = await openCard("running");
    await within(element).findByTestId("indexer-list");
    const result = await axe.run(element, {
      rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
    });
    expect(result.violations.map((v) => `${v.id}: ${v.nodes[0]?.html.slice(0, 80)}`)).toEqual([]);
  });
});

describe("tracker languages", () => {
  it("starts on English with the metadata language", async () => {
    await openCard("running");
    act(() => {
      useSettingsStore.getState().set("metadata.language", "de");
      useSettingsStore.getState().set("metadata.nfo-language", "tr");
    });
    const box = await screen.findByTestId("tracker-languages");
    const pressed = within(box)
      .getAllByRole("button", { pressed: true })
      .map((chip) => chip.textContent);
    expect(pressed).toEqual(["English", "German", "Turkish"]);
    expect(
      useSettingsStore.getState().values["torrents.indexer-service.languages"],
    ).toBeUndefined();
  });

  it("says how many indexers other languages hold, on each chip and in one line that turns them on", async () => {
    const user = userEvent.setup();
    const { torrents } = await openCard("running");
    const box = await screen.findByTestId("tracker-languages");
    const line = await within(box).findByTestId("tracker-languages-off");
    expect(line).toHaveTextContent("16 indexers in other languages are off.");
    expect(within(box).getByRole("button", { name: "Russian, 7 indexers" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    await user.click(within(line).getByRole("button", { name: "Turn them on" }));
    expect(useSettingsStore.getState().values["torrents.indexer-service.languages"]).toEqual([
      "en",
      "ru",
      "zh",
      "es",
      "fr",
    ]);
    expect(within(box).queryByTestId("tracker-languages-off")).toBeNull();
    // The change is checked at once, so the new sites join the search.
    await waitFor(() => expect(torrents.calls.indexer).toContain("check"), { timeout: 3000 });
  });

  it("switches another language on, and back to the default", async () => {
    const user = userEvent.setup();
    await openCard("running");
    const box = await screen.findByTestId("tracker-languages");
    await user.click(within(box).getByRole("button", { name: "Russian, 7 indexers" }));
    expect(useSettingsStore.getState().values["torrents.indexer-service.languages"]).toEqual([
      "en",
      "ru",
    ]);
    await user.click(within(box).getByRole("button", { name: "Russian, 7 indexers" }));
    expect(
      useSettingsStore.getState().values["torrents.indexer-service.languages"],
    ).toBeUndefined();
    // The last language cannot be switched off.
    expect(within(box).getByRole("button", { name: "English" })).toBeDisabled();
  });
});

describe("tracker language codes", () => {
  it("reads the language part of the codes", async () => {
    const { defaultTrackerLanguages, readTrackerLanguages } =
      await import("./trackerLanguageCodes");
    expect(
      defaultTrackerLanguages({
        "metadata.language": "pt-BR",
        "metadata.nfo-language": "tr;de",
      }),
    ).toEqual(["en", "de", "pt", "tr"]);
    // Every language set anywhere, lists and three-letter codes included, as the app reads them.
    expect(
      defaultTrackerLanguages({
        "metadata.fallback-language": "en-US",
        "metadata.nfo-language": ["tur", "*", "FR"],
        "metadata.language": "ger, rus, es-MX",
      }),
    ).toEqual(["en", "de", "es", "fr", "ru", "tr"]);
    // A code the app reported and this list cannot map is kept too.
    expect(defaultTrackerLanguages({}, ["en", "ka"])).toEqual(["en", "ka"]);
    expect(readTrackerLanguages(["RU", "x", 3, "en"])).toEqual(["ru", "en"]);
    expect(readTrackerLanguages([])).toBeNull();
    expect(readTrackerLanguages("en")).toBeNull();
  });
});
