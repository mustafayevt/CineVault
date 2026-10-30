import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import { transport } from "../../lib/transport";
import { requestFolderPick } from "../pickers/folderPickerRequest";
import type {
  FileExampleDto,
  FolderAnalysisDto,
  FolderChildDto,
  FolderGuessDto,
  OnboardingCommandError,
  OrganizeCommandError,
  SuggestedKindDto,
} from "../../bindings";
import { t } from "../../i18n/catalog";

export type FolderAnalysis = FolderAnalysisDto;
export type FolderChild = FolderChildDto;
export type FolderGuess = FolderGuessDto;
export type SuggestedKind = SuggestedKindDto;
/** A movie file name under one naming profile: what it is now and what it would become. */
export type NameExample = FileExampleDto;

/** A refusal from the backend: `message` is ready to show. */
export class OnboardingRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "OnboardingRequestError";
    this.code = code;
  }
}

/** What the first-run setup asks of the machine: folder contents, installed players, a picker. */
export interface OnboardingBackend {
  /** False in a plain browser, where there are no folders to look into. */
  readonly available: boolean;
  /** Looks inside a folder and says whether it holds movies, TV shows or both. */
  analyze: (path: string) => Promise<FolderAnalysis>;
  /** Opens the system folder picker. Empty when the user cancels. */
  pickFolders: () => Promise<string[]>;
  /**
   * Renders a naming profile on a video file name, or on a built-in sample when there is none.
   * Nothing is read from disk and nothing is saved.
   */
  nameExample: (profileId: string, fileName: string | null) => Promise<NameExample>;
}

type Result<T, E = OnboardingCommandError> =
  { status: "ok"; data: T } | { status: "error"; error: E };

function unwrap<T, E extends { code: string; message: string }>(result: Result<T, E>): T {
  if (result.status === "error") {
    throw new OnboardingRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: OnboardingBackend = {
  available: true,
  analyze: async (path) => unwrap(await commands.analyzeLibraryFolder(path)),
  pickFolders: () => commands.pickLibraryFolders(),
  nameExample: async (profileId, fileName) =>
    unwrap<NameExample, OrganizeCommandError>(
      await commands.organizeExampleForFile(profileId, fileName),
    ),
};

/** In a browser the server folder browser replaces the native picker; the rest is the server. */
const browserBackend: OnboardingBackend = {
  available: true,
  analyze: tauriBackend.analyze,
  pickFolders: () => requestFolderPick(),
  nameExample: tauriBackend.nameExample,
};

/** The backend of a plain browser (development): nothing can be looked into or picked. */
const unavailableBackend: OnboardingBackend = {
  available: false,
  analyze: () =>
    Promise.reject(new OnboardingRequestError("unavailable", t("onboarding.desktopOnly"))),
  pickFolders: () => Promise.resolve([]),
  nameExample: () =>
    Promise.reject(new OnboardingRequestError("unavailable", t("onboarding.desktopOnly"))),
};

let override: OnboardingBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setOnboardingBackend(backend: OnboardingBackend | null): void {
  override = backend;
}

let synthetic: OnboardingBackend | null = null;

/** Whether the address asks for the first-run setup in a plain browser (`?first-run`). */
export function firstRunPreview(): boolean {
  return (
    typeof window !== "undefined" && new URLSearchParams(window.location.search).has("first-run")
  );
}

/**
 * What `?first-run&step=2&folders=3` asks for in a plain browser: the step to open on and how
 * many sample folders to start with (the fourth is the mixed one, the fifth a single show). Used
 * to look at each screen.
 */
export function previewStart(): { step: 1 | 2 | 3 | 4; folders: string[] } | null {
  if (!firstRunPreview() || isTauri()) return null;
  const params = new URLSearchParams(window.location.search);
  const step = Number(params.get("step"));
  const count = Math.min(Math.max(Number(params.get("folders")) || 0, 0), PREVIEW_FOLDERS.length);
  return {
    step: step === 2 || step === 3 || step === 4 ? step : 1,
    folders: PREVIEW_FOLDERS.slice(0, count).map((folder) => folder.path),
  };
}

export function currentOnboardingBackend(): OnboardingBackend {
  if (override !== null) return override;
  if (transport() === "desktop") return tauriBackend;
  if (transport() === "browser") return browserBackend;
  if (firstRunPreview()) {
    synthetic ??= createSyntheticOnboardingBackend();
    return synthetic;
  }
  return unavailableBackend;
}

function leaf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function child(path: string, guess: FolderGuess, videos: number): FolderChild {
  return {
    path,
    name: leaf(path),
    guess,
    suggested: guess === "tv" ? "tv" : "movie",
    videos,
    show: false,
  };
}

function analysis(
  path: string,
  guess: FolderGuess,
  videos: number,
  episodes: number,
  suggested?: SuggestedKind,
): FolderAnalysis {
  const fallback: SuggestedKind = guess === "tv" ? "tv" : guess === "mixed" ? "undecided" : "movie";
  return {
    path,
    name: leaf(path),
    guess,
    suggested: suggested ?? fallback,
    videos,
    episodes,
    truncated: false,
    children: [],
    loose_videos: 0,
    loose_examples: [],
    single_show: false,
    first_video: guess === "movies" ? "Heat.1995.1080p.BluRay.x264-GRP.mkv" : null,
  };
}

/** Folders a browser preview offers, one per click on the drop zone. */
const SAMPLE_FOLDERS: readonly FolderAnalysis[] = [
  analysis("D:\\Media\\Movies", "movies", 134, 0),
  analysis("F:\\Personal\\Movie", "movies", 138, 3),
  analysis("D:\\Media\\TVShows", "empty", 0, 0, "tv"),
];

/** A mixed folder, for trying the split in a browser (it is the fourth sample). */
const SAMPLE_MIXED: FolderAnalysis = {
  ...analysis("E:\\Media", "mixed", 60, 24),
  children: [child("E:\\Media\\Films", "movies", 36), child("E:\\Media\\Series", "tv", 24)],
  loose_videos: 2,
  loose_examples: ["Arrival (2016).mkv", "Zodiac (2007).mkv"],
};

/** One show picked by itself, for trying "Use parent" in a browser (it is the fifth sample). */
const SAMPLE_SINGLE_SHOW: FolderAnalysis = {
  ...analysis("E:\\Downloads\\Chernobyl (2019)", "tv", 5, 5),
  single_show: true,
};

/** The file name the sample example starts from, as the desktop app uses it. */
const SAMPLE_FILE = "Dune.Part.Two.2024.2160p.WEB-DL.DV.HDR10.TrueHD.Atmos.7.1.mkv";

/**
 * A rough stand-in for the desktop app's renderer, for a plain browser: it reads the title and
 * year from the name and adds the tags of the profile. The desktop app renders the real templates.
 */
export function syntheticNameExample(profileId: string, fileName: string | null): NameExample {
  const given = fileName !== null && fileName.trim() !== "";
  const current = given ? fileName.trim() : SAMPLE_FILE;
  const stem = current.replace(/\.[A-Za-z0-9]{2,4}$/, "");
  const ext = current.slice(stem.length);
  const found = /^(.*?)[. _(]+((?:19|20)\d{2})(?!\d)/.exec(stem);
  const title = (found?.[1] ?? stem).replace(/[._]+/g, " ").trim();
  const year = found?.[2] ?? "";
  const base = year === "" ? title : `${title} (${year})`;
  const tags = [
    /2160p|4k/i.test(stem) ? "2160p" : /1080p/i.test(stem) ? "1080p" : null,
    /blu-?ray/i.test(stem) ? "Blu-ray" : /web-?dl/i.test(stem) ? "Web-DL" : null,
  ].filter((tag): tag is string => tag !== null);
  const tmdb = /tmdb-?(\d+)/i.exec(stem)?.[1];
  const named =
    profileId === "detailed"
      ? `${base}${tags.map((tag) => ` [${tag}]`).join("")}`
      : profileId === "media-server-safe"
        ? `${base}${tmdb === undefined ? "" : ` {tmdb-${tmdb}}`}`
        : base;
  return {
    current,
    folder: named,
    file: `${named}${ext}`,
    title,
    year: Number(year) || 0,
    sample: !given,
  };
}

/** The parent of the single show, a folder of three shows, for trying "Use parent". */
const SAMPLE_DOWNLOADS: FolderAnalysis = analysis("E:\\Downloads", "tv", 12, 12);

/** The folders a preview starts with, in the order `folders=N` takes them. */
const PREVIEW_FOLDERS: readonly FolderAnalysis[] = [
  ...SAMPLE_FOLDERS,
  SAMPLE_MIXED,
  SAMPLE_SINGLE_SHOW,
];

/**
 * A stand-in for development in a plain browser: the drop zone hands out the sample folders one
 * after the other (the mixed one and the single show last), and any other path is a small movie
 * folder.
 */
function createSyntheticOnboardingBackend(): OnboardingBackend {
  const samples = PREVIEW_FOLDERS;
  let next = 0;
  return {
    available: true,
    analyze: (path) =>
      Promise.resolve(
        [...samples, SAMPLE_DOWNLOADS].find((folder) => folder.path === path) ??
          analysis(path, "movies", 12, 0),
      ),
    pickFolders: () => {
      const folder = samples[next % samples.length];
      next += 1;
      return Promise.resolve(folder === undefined ? [] : [folder.path]);
    },
    nameExample: (profileId, fileName) =>
      Promise.resolve(syntheticNameExample(profileId, fileName)),
  };
}
