import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { SourceRequestError, setSourcesBackend } from "../library-sources/sourcesBackend";
import type { LibrarySource, SourcesBackend } from "../library-sources/sourcesBackend";
import { setOnboardingBackend } from "../onboarding/onboardingBackend";
import type { OnboardingBackend } from "../onboarding/onboardingBackend";
import { setOrganizeBackend } from "../rename/organizeBackend";
import { createSyntheticOrganizeBackend } from "../rename/syntheticOrganize";
import { setProvidersBackend, unavailableBackend } from "../providers/providersBackend";
import { setScanBackend } from "../scan/scanBackend";
import type { ScanBackend } from "../scan/scanBackend";
import { useToastStore } from "../toasts/toastStore";
import { setUpdatesBackend } from "../updates/updatesBackend";
import type { UpdatesBackend } from "../updates/updatesBackend";
import { useUpdateStore } from "../updates/updateStore";
import { SettingsPage } from "./SettingsPage";
import { settleSettingSaves, useSettingsStore } from "./settingsStore";
import { stepAppZoom } from "./zoom";

async function renderSettings() {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({ getParentRoute: () => root, path: "/settings", component: SettingsPage }),
    ]),
    history: createMemoryHistory({ initialEntries: ["/settings"] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByRole("heading", { level: 1, name: "Settings" });
}

beforeEach(() => {
  setOrganizeBackend(createSyntheticOrganizeBackend());
  setProvidersBackend(unavailableBackend);
});

afterEach(() => {
  setOrganizeBackend(null);
  setProvidersBackend(null);
  setSourcesBackend(null);
  setScanBackend(null);
  setOnboardingBackend(null);
  setUpdatesBackend(null);
  useUpdateStore.setState({ lastCheckedAt: null, checkResult: null, phase: "idle", offer: null });
});

/** The messages of the toasts shown so far. */
const toasts = () => useToastStore.getState().toasts.map((toast) => toast.message);

const folder = (id: number, kind: "movie" | "tv", path: string): LibrarySource => ({
  id,
  kind,
  path,
  label: path,
  position: id,
});

/** Library folders held in memory, with the calls recorded. */
function folders(initial: LibrarySource[], refuseKind?: string) {
  let list = initial;
  const calls: string[] = [];
  const backend: SourcesBackend = {
    available: true,
    list: () => Promise.resolve([...list]),
    add: (kind, path) => {
      calls.push(`add ${kind} ${path}`);
      const created = folder(list.length + 10, kind, path);
      list = [...list, created];
      return Promise.resolve(created);
    },
    remove: (id) => {
      calls.push(`remove ${id}`);
      const source = list.find((entry) => entry.id === id) as LibrarySource;
      list = list.filter((entry) => entry.id !== id);
      return Promise.resolve({ source, kept_items: 5 });
    },
    reorder: () => Promise.resolve([...list]),
    remap: () => Promise.reject(new Error("no")),
    rename: () => Promise.reject(new Error("no")),
    setKind: (id, kind) => {
      calls.push(`kind ${id} ${kind}`);
      if (refuseKind !== undefined) {
        return Promise.reject(new SourceRequestError("inside_existing", refuseKind));
      }
      const changed = { ...(list.find((entry) => entry.id === id) as LibrarySource), kind };
      list = list.map((entry) => (entry.id === id ? changed : entry));
      return Promise.resolve(changed);
    },
  };
  const scan: ScanBackend = {
    available: false,
    rescan: () => Promise.resolve({ job_id: 0, sources: 0, media_info: "ready" }),
    findMissing: () => Promise.resolve([]),
    sourceHealth: () =>
      Promise.resolve(
        list.map((entry) => ({
          id: entry.id,
          kind: entry.kind,
          path: entry.path,
          online: !entry.path.startsWith("Z:"),
          titles: entry.kind === "tv" ? 18 : 142,
          missing: 0,
          pending_media_info: 0,
        })),
      ),
    readPendingMediaInfo: () => Promise.resolve("ready"),
  };
  setSourcesBackend(backend);
  setScanBackend(scan);
  return { calls };
}

describe("Settings, Library folders", () => {
  const SAMPLE = [
    folder(1, "movie", "D:\\Movies"),
    folder(2, "tv", "E:\\TV"),
    folder(3, "tv", "Z:\\Shows"),
  ];

  it("lists each folder with what it holds, whether it is watched or offline, and its type", async () => {
    folders(SAMPLE);
    await renderSettings();
    const movies = await screen.findByRole("group", { name: "D:\\Movies holds" });
    expect(within(movies).getByRole("button", { name: "Movies" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(await screen.findByText("142 titles · watched for changes")).toBeVisible();
    expect(screen.getByText("18 shows · watched for changes")).toBeVisible();
    expect(screen.getByText("offline · 18 shows kept")).toBeVisible();
    const tv = screen.getByRole("group", { name: "E:\\TV holds" });
    expect(within(tv).getByRole("button", { name: "TV shows" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("adds a folder from the picker with the type it looks like", async () => {
    const user = userEvent.setup();
    const state = folders(SAMPLE);
    const onboarding: OnboardingBackend = {
      available: true,
      analyze: (path) =>
        Promise.resolve({
          path,
          name: "Series",
          guess: "tv",
          suggested: "tv",
          videos: 12,
          episodes: 12,
          truncated: false,
          children: [],
          loose_videos: 0,
          loose_examples: [],
          single_show: false,
          first_video: null,
        }),
      pickFolders: () => Promise.resolve(["F:\\Series"]),
      nameExample: () => Promise.reject(new Error("no")),
    };
    setOnboardingBackend(onboarding);
    await renderSettings();
    await user.click(await screen.findByRole("button", { name: "Add folder…" }));
    await waitFor(() => {
      expect(state.calls).toContain("add tv F:\\Series");
    });
    expect(await screen.findByRole("group", { name: "F:\\Series holds" })).toBeVisible();
    await waitFor(() => {
      expect(toasts()).toContain("Added 1 folder");
    });
  });

  it("removes a folder, keeps its titles and offers Undo", async () => {
    const user = userEvent.setup();
    const state = folders(SAMPLE);
    await renderSettings();
    await user.click(await screen.findByRole("button", { name: "Remove D:\\Movies" }));
    await waitFor(() => {
      expect(state.calls).toContain("remove 1");
    });
    await waitFor(() => {
      expect(toasts()).toContain("Removed D:\\Movies · 5 titles stay, offline");
    });
    const entry = useToastStore.getState().toasts[0];
    await act(async () => {
      await entry?.undo?.();
    });
    expect(state.calls).toContain("add movie D:\\Movies");
  });

  it("changes what a folder holds", async () => {
    const user = userEvent.setup();
    const state = folders(SAMPLE);
    await renderSettings();
    const movies = await screen.findByRole("group", { name: "D:\\Movies holds" });
    await user.click(within(movies).getByRole("button", { name: "TV shows" }));
    await waitFor(() => {
      expect(state.calls).toContain("kind 1 tv");
    });
    await waitFor(() => {
      expect(
        within(screen.getByRole("group", { name: "D:\\Movies holds" })).getByRole("button", {
          name: "TV shows",
        }),
      ).toHaveAttribute("aria-pressed", "true");
    });
  });

  it("puts the type back and says why when it cannot be changed", async () => {
    const user = userEvent.setup();
    folders(SAMPLE, "D:\\Movies is inside a TV folder.");
    await renderSettings();
    const movies = await screen.findByRole("group", { name: "D:\\Movies holds" });
    await user.click(within(movies).getByRole("button", { name: "TV shows" }));
    await waitFor(() => {
      expect(toasts()).toContain("D:\\Movies is inside a TV folder.");
    });
    await waitFor(() => {
      expect(within(movies).getByRole("button", { name: "Movies" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
  });
});

describe("Settings, Appearance", () => {
  it("offers Light, Dark and Match Windows and applies the choice at once", async () => {
    const user = userEvent.setup();
    await renderSettings();
    await act(() => useSettingsStore.getState().load());
    const theme = await screen.findByRole("group", { name: "Theme" });
    expect(within(theme).getByRole("button", { name: "Dark" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(within(theme).getByRole("button", { name: "Light" }));
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["appearance.theme"]).toBe("light");
    await user.click(within(theme).getByRole("button", { name: "Match Windows" }));
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["appearance.theme"]).toBe("system");
  });
});

describe("Settings, Appearance, Size", () => {
  it("shows the app zoom, changes it at once and follows the zoom keys", async () => {
    const user = userEvent.setup();
    await renderSettings();
    await act(() => useSettingsStore.getState().load());
    const size = await screen.findByRole("combobox", { name: "Size" });
    expect(size).toHaveValue("100");
    expect([...(size as HTMLSelectElement).options].map((option) => option.textContent)).toEqual([
      "80%",
      "90%",
      "100%",
      "110%",
      "125%",
      "150%",
    ]);
    await user.selectOptions(size, "125");
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["appearance.zoom"]).toBe(125);
    // A keyboard change shows in the control.
    act(() => stepAppZoom("in"));
    expect(screen.getByRole("combobox", { name: "Size" })).toHaveValue("150");
    act(() => stepAppZoom("reset"));
    expect(screen.getByRole("combobox", { name: "Size" })).toHaveValue("100");
    await settleSettingSaves();
  });
});

describe("Settings, Updates", () => {
  function updates(found: boolean): UpdatesBackend {
    return {
      available: true,
      status: () =>
        Promise.resolve({
          current_version: "0.2.5",
          channel_set: true,
          enabled: true,
          offer: null,
        }),
      check: () =>
        Promise.resolve({
          outcome: found ? "available" : "up_to_date",
          offer: found
            ? { version: "0.2.6", current: "0.2.5", notes: "", size_bytes: 1000, published: "" }
            : null,
        } as Awaited<ReturnType<UpdatesBackend["check"]>>),
      install: () => Promise.resolve({ restarting: true, reason: null }),
      cancel: () => Promise.resolve(),
      blockingJobs: () => Promise.resolve(0),
    };
  }

  it("shows the version and, after Check now, when the app was last found up to date", async () => {
    const user = userEvent.setup();
    setUpdatesBackend(updates(false));
    await renderSettings();
    expect(await screen.findByText("CineVault 0.2.5")).toBeVisible();
    expect(screen.getByText("Not checked yet")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Check now" }));
    expect(await screen.findByText(/^Up to date · checked today at /)).toBeVisible();
  });

  it("offers the update it found", async () => {
    const user = userEvent.setup();
    setUpdatesBackend(updates(true));
    await renderSettings();
    await user.click(await screen.findByRole("button", { name: "Check now" }));
    expect(
      await screen.findByText(/^CineVault 0\.2\.6 is available · checked today at /),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Update now" })).toBeVisible();
  });

  it("keeps Install updates when I close the app off until it is switched on", async () => {
    const user = userEvent.setup();
    setUpdatesBackend(updates(false));
    await renderSettings();
    await act(() => useSettingsStore.getState().load());
    const option = await screen.findByRole("switch", {
      name: "Install updates when I close the app",
    });
    expect(option).not.toBeChecked();
    await user.click(option);
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["updates.install-on-close"]).toBe(true);
    expect(option).toBeChecked();
  });
});
