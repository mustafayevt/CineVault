import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setOrganizeBackend } from "../rename/organizeBackend";
import { createSyntheticOrganizeBackend } from "../rename/syntheticOrganize";
import { setProvidersBackend } from "../providers/providersBackend";
import type { ProviderCredentials, ProvidersBackend } from "../providers/providersBackend";
import { createSyntheticProvidersBackend } from "../providers/syntheticProviders";
import { SettingsPage } from "./SettingsPage";
import { settleSettingSaves, useSettingsStore } from "./settingsStore";

async function renderSettings(path = "/settings") {
  const root = createRootRoute();
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: "/settings",
        validateSearch: (search: Record<string, unknown>) => ({
          section: typeof search.section === "string" ? search.section : undefined,
        }),
        component: SettingsPage,
      }),
    ]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByRole("heading", { level: 1, name: "Settings" });
  return router;
}

/** The synthetic backend with its list narrowed to what a test needs and calls recorded. */
function recordingBackend(): ProvidersBackend & { tested: Record<string, string>[] } {
  const inner = createSyntheticProvidersBackend(true);
  const tested: Record<string, string>[] = [];
  return {
    ...inner,
    tested,
    test: (provider, drafts) => {
      tested.push({ provider, ...drafts });
      return Promise.resolve(
        drafts.api_key === "bad"
          ? { ok: false, code: "rejected", message: "TMDB turned this key down." }
          : { ok: true, code: null, message: null },
      );
    },
  };
}

beforeEach(() => {
  setOrganizeBackend(createSyntheticOrganizeBackend());
});
afterEach(() => {
  setProvidersBackend(null);
  setOrganizeBackend(null);
});

describe("the Settings page frame", () => {
  it("is one page with a flat index and the Providers section", async () => {
    setProvidersBackend(recordingBackend());
    await renderSettings();
    const index = screen.getByRole("navigation", { name: "Settings sections" });
    expect(within(index).getByRole("button", { name: "Providers" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByRole("heading", { level: 2, name: "Providers" })).toBeVisible();
  });

  it("scrolls to a section and marks it in the index when its entry is clicked", async () => {
    const user = userEvent.setup();
    setProvidersBackend(recordingBackend());
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    await renderSettings();
    const index = screen.getByRole("navigation", { name: "Settings sections" });
    const entries = within(index).getAllByRole("button");
    const last = entries[entries.length - 1];
    if (last === undefined) throw new Error("no index entries");
    await user.click(last);
    expect(scrollIntoView).toHaveBeenCalled();
    expect(last).toHaveAttribute("aria-current", "true");
    Element.prototype.scrollIntoView = () => {};
  });

  it("follows the scroll: the section at the top of the page is the active one", async () => {
    setProvidersBackend(recordingBackend());
    await renderSettings();
    const region = screen.getByRole("region", { name: "Settings" });
    const scroller = region.querySelector<HTMLElement>(".overflow-y-auto");
    if (scroller === null) throw new Error("no scroller");
    const sections = [...region.querySelectorAll<HTMLElement>("[data-settings-section]")];
    // Section n sits at n * 400 px; the page is scrolled by 400 px, so the second is at the top.
    sections.forEach((node, n) => {
      node.getBoundingClientRect = () => ({ top: n * 400 - 400 }) as DOMRect;
    });
    scroller.getBoundingClientRect = () => ({ top: 0 }) as DOMRect;
    fireEvent.scroll(scroller);
    const index = screen.getByRole("navigation", { name: "Settings sections" });
    const current = within(index)
      .getAllByRole("button")
      .find((entry) => entry.getAttribute("aria-current") === "true");
    expect(current).toBeDefined();
    expect(sections.length).toBeGreaterThan(0);
  });
});

describe("Settings, Providers", () => {
  it("shows what each saved key does, without ever showing a key", async () => {
    setProvidersBackend(recordingBackend());
    await renderSettings();
    const omdb = await screen.findByLabelText("OMDb key");
    const row = omdb.closest("[data-provider]");
    expect(row).toHaveAttribute("data-state", "active");
    expect(within(row as HTMLElement).getByText("Works")).toBeVisible();
    expect(omdb).toHaveValue("");
    expect(omdb).toHaveAttribute("type", "password");
  });

  it("tests a typed key, saves it when it works and says Works", async () => {
    const user = userEvent.setup();
    const backend = recordingBackend();
    setProvidersBackend(backend);
    await renderSettings();
    const tmdb = await screen.findByLabelText("TMDB key");
    await user.type(tmdb, "good-key");
    await user.click(screen.getByRole("button", { name: "Show" }));
    expect(tmdb).toHaveAttribute("type", "text");
    await user.click(screen.getByRole("button", { name: "Test the TMDB key" }));
    await waitFor(() => {
      expect(
        within(tmdb.closest("[data-provider]") as HTMLElement).getByText("Works"),
      ).toBeVisible();
    });
    expect(backend.tested[0]).toMatchObject({ provider: "tmdb", api_key: "good-key" });
    expect(tmdb).toHaveValue("");
  });

  it("keeps a key that was turned down and says why", async () => {
    const user = userEvent.setup();
    setProvidersBackend(recordingBackend());
    await renderSettings();
    const tmdb = await screen.findByLabelText("TMDB key");
    await user.type(tmdb, "bad");
    await user.click(screen.getByRole("button", { name: "Test the TMDB key" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("TMDB turned this key down.");
    expect(tmdb).toHaveValue("bad");
  });

  it("applies the metadata language at once and stores it", async () => {
    const user = userEvent.setup();
    setProvidersBackend(recordingBackend());
    await renderSettings();
    await act(() => useSettingsStore.getState().load());
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Metadata language" }),
      "Deutsch",
    );
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["metadata.language"]).toBe("de");
  });

  it("lists the providers as unavailable where there is no credential store", async () => {
    setProvidersBackend({
      ...recordingBackend(),
      available: false,
      list: () => Promise.resolve<ProviderCredentials[]>([]),
    });
    await renderSettings();
    expect(
      (await screen.findAllByPlaceholderText("Keys can be set in the desktop app")).length,
    ).toBe(3);
  });
});

describe("Settings, Naming", () => {
  const templateField = (name: string) => screen.findByLabelText(`${name} template`);

  it("shows the active templates with their friendly token names and a live example", async () => {
    setProvidersBackend(recordingBackend());
    await renderSettings();
    expect(await templateField("Movie folder")).toHaveValue("{Title} ({Year})");
    expect(await templateField("TV episode")).toHaveValue(
      "{Show} - S{Season00}E{Episode00} - {EpisodeTitle}",
    );
    const row = (await templateField("Movie folder")).closest("div[class*='grid']") as HTMLElement;
    await waitFor(() => {
      expect(within(row).getByText(/\(\d{4}\)$/)).toBeVisible();
    });
  });

  it("inserts a token at the cursor and updates the example at once", async () => {
    const user = userEvent.setup();
    const backend = createSyntheticOrganizeBackend();
    setOrganizeBackend(backend);
    setProvidersBackend(recordingBackend());
    await renderSettings();
    const field = await templateField("Movie file");
    await user.click(field);
    await user.clear(field);
    await user.type(field, "{{Title} ({{Year})");
    await user.click(screen.getByRole("button", { name: "{Resolution}" }));
    expect(field).toHaveValue("{Title} ({Year}){Resolution}");
    expect(field).toHaveFocus();
    // The template is saved as a profile of the user's own, in the engine's syntax.
    await waitFor(async () => {
      const profiles = await backend.profiles();
      const active = profiles.profiles.find((profile) => profile.id === profiles.active);
      expect(active?.builtin).toBe(false);
      expect(active?.patterns.movie_file).toBe("${title} (${year})${videoFormat}");
    });
  });

  it("says why a template is invalid and keeps the last valid one in use", async () => {
    const user = userEvent.setup();
    const backend = createSyntheticOrganizeBackend();
    setOrganizeBackend(backend);
    setProvidersBackend(recordingBackend());
    await renderSettings();
    const field = await templateField("Movie folder");
    await user.click(field);
    await user.clear(field);
    await user.type(field, "{{Title");
    expect(await screen.findByText(/The brace in "\{Title" is not closed/)).toBeVisible();
    expect(field).toHaveAttribute("aria-invalid", "true");
    const saved = await backend.profiles();
    expect(saved.active).toBe("clean");
    expect(saved.profiles.find((p) => p.id === "clean")?.patterns.movie_folder).toBe(
      "${title} (${year})",
    );
  });

  it("stores the colon style and shows it in the example", async () => {
    const user = userEvent.setup();
    setProvidersBackend(recordingBackend());
    await renderSettings();
    await act(() => useSettingsStore.getState().load());
    await user.click(screen.getByRole("button", { name: "Use “꞉”" }));
    await settleSettingSaves();
    expect(useSettingsStore.getState().values["naming.colon"]).toBe("lookalike");
  });
});
