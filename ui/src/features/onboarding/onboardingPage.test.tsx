import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setSourcesBackend } from "../library-sources/sourcesBackend";
import type { LibrarySource, SourcesBackend } from "../library-sources/sourcesBackend";
import { setProvidersBackend } from "../providers/providersBackend";
import { createSyntheticProvidersBackend } from "../providers/syntheticProviders";
import { setOrganizeBackend } from "../rename/organizeBackend";
import { createSyntheticOrganizeBackend } from "../rename/syntheticOrganize";
import { setScanBackend } from "../scan/scanBackend";
import type { ScanBackend } from "../scan/scanBackend";
import { settleSettingSaves, useSettingsStore } from "../settings/settingsStore";
import { OnboardingPage, ONBOARDING_DONE_KEY } from "./OnboardingPage";
import { setOnboardingBackend, syntheticNameExample } from "./onboardingBackend";
import type { FolderAnalysis, OnboardingBackend } from "./onboardingBackend";

function analysis(path: string, suggested: "movie" | "tv", videos: number): FolderAnalysis {
  return {
    path,
    name: path.split("\\").pop() ?? path,
    guess: suggested === "tv" ? "tv" : "movies",
    suggested,
    videos,
    episodes: suggested === "tv" ? videos : 0,
    truncated: false,
    children: [],
    loose_videos: 0,
    loose_examples: [],
    single_show: false,
    first_video: suggested === "movie" ? "Heat.1995.1080p.BluRay.mkv" : null,
  };
}

const FOLDERS: Record<string, FolderAnalysis> = {
  "D:\\Movies": analysis("D:\\Movies", "movie", 146),
  "E:\\TV": analysis("E:\\TV", "tv", 18),
};

/** Everything the setup talks to, held in memory with the calls recorded. */
function backends(picks: string[]) {
  const added: { kind: string; path: string }[] = [];
  const rescans = vi.fn(() =>
    Promise.resolve({ job_id: 1, sources: 1, media_info: "ready" as const }),
  );
  let list: LibrarySource[] = [];
  const sources: SourcesBackend = {
    available: true,
    list: () => Promise.resolve([...list]),
    add: (kind, path) => {
      added.push({ kind, path });
      const created: LibrarySource = { id: list.length + 1, kind, path, label: path, position: 0 };
      list = [...list, created];
      return Promise.resolve(created);
    },
    remove: () => Promise.reject(new Error("no")),
    reorder: () => Promise.resolve([...list]),
    remap: () => Promise.reject(new Error("no")),
    rename: () => Promise.reject(new Error("no")),
    setKind: () => Promise.reject(new Error("no")),
  };
  const onboarding: OnboardingBackend = {
    available: true,
    analyze: (path) => Promise.resolve(FOLDERS[path] ?? analysis(path, "movie", 1)),
    pickFolders: () => Promise.resolve(picks.splice(0, 1)),
    nameExample: (profile, file) => Promise.resolve(syntheticNameExample(profile, file)),
  };
  const scan: ScanBackend = {
    available: true,
    rescan: rescans,
    findMissing: () => Promise.resolve([]),
    sourceHealth: () => Promise.resolve([]),
    readPendingMediaInfo: () => Promise.resolve("ready"),
  };
  setSourcesBackend(sources);
  setOnboardingBackend(onboarding);
  setScanBackend(scan);
  setProvidersBackend(createSyntheticProvidersBackend(false));
  setOrganizeBackend(createSyntheticOrganizeBackend());
  return { added, rescans };
}

afterEach(() => {
  setSourcesBackend(null);
  setOnboardingBackend(null);
  setScanBackend(null);
  setProvidersBackend(null);
  setOrganizeBackend(null);
});

beforeEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});

const footer = () =>
  screen.getByRole("button", { name: "Skip setup" }).parentElement as HTMLElement;
const heading = (name: string) => screen.findByRole("heading", { level: 1, name });

describe("the first-run setup", () => {
  it("has four numbered steps, a footer with the three actions and the note about Settings", async () => {
    backends([]);
    render(<OnboardingPage onFinished={() => undefined} windowApi={null} />);
    const steps = screen.getByRole("list", { name: "Setup steps" });
    expect(
      within(steps)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["1Folders", "2TMDB key", "3Ratings keys", "4Naming"]);
    expect(within(footer()).getByRole("button", { name: "Skip this step" })).toBeVisible();
    expect(within(footer()).getByRole("button", { name: "Continue" })).toBeVisible();
    expect(screen.getByText("Every step can be changed later in Settings.")).toBeVisible();
    expect(await heading("Where are your movies and shows?")).toBeVisible();
  });

  it("lists an added folder with what was found, the detected type and a switch", async () => {
    const user = userEvent.setup();
    backends(["E:\\TV"]);
    render(<OnboardingPage onFinished={() => undefined} windowApi={null} />);
    const continueButton = screen.getByRole("button", { name: "Continue" });
    expect(continueButton).toHaveAttribute("aria-disabled", "true");
    await user.click(screen.getByRole("button", { name: "Add folder…" }));
    const holds = await screen.findByRole("group", { name: "E:\\TV holds" });
    expect(screen.getByText("18 videos found · looks like TV shows")).toBeVisible();
    expect(within(holds).getByRole("button", { name: "TV shows" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(within(holds).getByRole("button", { name: "Movies" }));
    expect(within(holds).getByRole("button", { name: "Movies" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Remove E:\\TV" }));
    expect(screen.queryByRole("group", { name: "E:\\TV holds" })).toBeNull();
  });

  it("finishes the minimal setup: a folder, a key that works, no ratings keys, the suggested naming", async () => {
    const user = userEvent.setup();
    const onFinished = vi.fn();
    const state = backends(["D:\\Movies"]);
    render(<OnboardingPage onFinished={onFinished} windowApi={null} />);
    await user.click(screen.getByRole("button", { name: "Add folder…" }));
    await screen.findByText("146 videos found");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await heading("Add your TMDB key");
    expect(screen.getByRole("button", { name: "Continue" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.type(screen.getByLabelText("TMDB key"), "a-valid-looking-key");
    await user.click(screen.getByRole("button", { name: "Test the TMDB key" }));
    expect(await screen.findByText("Works")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await heading("Add your ratings keys");
    await user.click(screen.getByRole("button", { name: "Skip this step" }));

    await heading("How should folders and files be named?");
    expect(await screen.findByText("Heat.1995.1080p.BluRay.mkv")).toBeVisible();
    expect(screen.getByText("Nothing is renamed until you ask on the Rename page.")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => {
      expect(onFinished).toHaveBeenCalledWith("movies");
    });
    expect(state.added).toEqual([{ kind: "movie", path: "D:\\Movies" }]);
    expect(state.rescans).toHaveBeenCalledTimes(1);
    await settleSettingSaves();
    expect(useSettingsStore.getState().values[ONBOARDING_DONE_KEY]).toBe(true);
  });

  it("opens the Library empty after Skip setup, adds nothing and does not show again", async () => {
    const user = userEvent.setup();
    const onFinished = vi.fn();
    const state = backends(["D:\\Movies"]);
    render(<OnboardingPage onFinished={onFinished} windowApi={null} />);
    await user.click(screen.getByRole("button", { name: "Add folder…" }));
    await screen.findByText("146 videos found");
    await user.click(screen.getByRole("button", { name: "Skip setup" }));
    await waitFor(() => {
      expect(onFinished).toHaveBeenCalled();
    });
    expect(state.added).toEqual([]);
    expect(state.rescans).not.toHaveBeenCalled();
    await settleSettingSaves();
    expect(useSettingsStore.getState().values[ONBOARDING_DONE_KEY]).toBe(true);
  });

  it("skips every step one by one without adding or changing anything", async () => {
    const user = userEvent.setup();
    const onFinished = vi.fn();
    const state = backends([]);
    render(<OnboardingPage onFinished={onFinished} windowApi={null} />);
    for (const title of [
      "Where are your movies and shows?",
      "Add your TMDB key",
      "Add your ratings keys",
      "How should folders and files be named?",
    ]) {
      await heading(title);
      await user.click(screen.getByRole("button", { name: "Skip this step" }));
    }
    await waitFor(() => {
      expect(onFinished).toHaveBeenCalled();
    });
    expect(state.added).toEqual([]);
    expect(state.rescans).not.toHaveBeenCalled();
  });

  it("makes the picked naming style the active one when the setup is finished", async () => {
    const user = userEvent.setup();
    backends([]);
    const organize = createSyntheticOrganizeBackend();
    setOrganizeBackend(organize);
    render(<OnboardingPage onFinished={() => undefined} windowApi={null} />);
    await user.click(screen.getByRole("button", { name: "Skip this step" }));
    await user.click(await screen.findByRole("button", { name: "Skip this step" }));
    await user.click(await screen.findByRole("button", { name: "Skip this step" }));
    await heading("How should folders and files be named?");
    const styles = screen.getByRole("group", { name: "Naming styles" });
    await user.click(within(styles).getByRole("button", { name: /Detailed/ }));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(async () => {
      expect((await organize.profiles()).active).toBe("detailed");
    });
  });
});
