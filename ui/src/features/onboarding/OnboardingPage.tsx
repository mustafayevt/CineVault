import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { OrganizeProfileDto } from "../../bindings";
import { BrandMark } from "../../components/BrandMark";
import { Button } from "../../components/Button";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { currentSourcesBackend } from "../library-sources/sourcesBackend";
import type { SourceKind } from "../library-sources/sourcesBackend";
import { useProviderKeys } from "../providers/useProviderKeys";
import { currentOrganizeBackend } from "../rename/organizeBackend";
import { runRescan } from "../scan/scanActions";
import { useAppearance } from "../settings/appearance";
import { useLocaleSettings } from "../settings/localeSettings";
import { settleSettingSaves, useSettingsStore } from "../settings/settingsStore";
import { CaptionButtons } from "../shell/CaptionButtons";
import { detectPlatform } from "../shell/platform";
import { tauriWindowApi, useWindowMaximized } from "../shell/windowApi";
import type { WindowApi } from "../shell/windowApi";
import { showToast } from "../toasts/toastStore";
import {
  failedEntry,
  folderStepReason,
  foldersToAdd,
  lookingEntry,
  readyEntry,
  readyToContinue,
  samePath,
} from "./folderEntries";
import type { FolderEntry } from "./folderEntries";
import {
  OnboardingRequestError,
  currentOnboardingBackend,
  previewStart,
} from "./onboardingBackend";
import { downloadTools, queueRecommendedTools } from "./setupTools";
import { FoldersStep, NamingStep, RatingsStep, Stepper, TmdbStep } from "./SetupSteps";
import type { Step } from "./SetupSteps";
import { markCurrentVersionSeen } from "../changelog/whatsNew";
import { useFolderDrop } from "./useFolderDrop";

/** The setting that remembers the first-run setup was finished or left. */
export const ONBOARDING_DONE_KEY = "onboarding.completed";

const LAST_STEP: Step = 4;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Why a folder cannot be used, in one sentence that names the path once. */
function analysisError(path: string, error: unknown): string {
  const unreadable = t("setup.folders.unreadable", { path });
  if (!(error instanceof OnboardingRequestError)) return `${unreadable} ${messageOf(error)}`;
  if (error.code === "not_a_folder") return unreadable;
  if (error.code === "storage" || error.code === "unavailable") {
    return `${unreadable} ${error.message}`;
  }
  return error.message;
}

interface FinishOptions {
  /** Add the folders of the first step. */
  folders: boolean;
  /** Make the chosen naming style the active one. */
  style: boolean;
}

interface OnboardingPageProps {
  /** Called when the setup is over, with the library to open. */
  onFinished: (target: "movies" | "tv") => void;
  /** The window to control; defaults to the current Tauri window (none in a plain browser). */
  windowApi?: WindowApi | null;
}

/**
 * The full-window first run (`/welcome`): four numbered steps on the window
 * ground, Folders, TMDB key, Ratings keys and Naming, each in a card with a footer that offers
 * Skip setup, Skip this step and Continue. Nothing is saved except a key that passed Test until the
 * last step: finishing adds the folders, makes the naming style active, starts the scan and the
 * download of the tools. Nothing is renamed, moved or deleted by any of it.
 */
export function OnboardingPage({ onFinished, windowApi }: OnboardingPageProps) {
  useAppearance();
  useLocaleSettings();
  const platform = useMemo(() => detectPlatform(), []);
  const api = useMemo(() => (windowApi === undefined ? tauriWindowApi() : windowApi), [windowApi]);
  const maximized = useWindowMaximized(api);
  const keys = useProviderKeys();

  const [start] = useState(previewStart);
  const [step, setStep] = useState<Step>(start?.step ?? 1);
  const [entries, setEntries] = useState<readonly FolderEntry[]>(() =>
    (start?.folders ?? []).map(lookingEntry),
  );
  const entriesRef = useRef(entries);
  const [profiles, setProfiles] = useState<readonly OrganizeProfileDto[]>([]);
  const [active, setActive] = useState<string | null>(null);
  // `null` until the user picks one: the active profile applies.
  const [chosen, setChosen] = useState<string | null>(null);
  const [building, setBuilding] = useState(false);
  const heading = useRef<HTMLHeadingElement>(null);
  const firstRender = useRef(true);

  useEffect(() => {
    let live = true;
    currentOrganizeBackend()
      .profiles()
      .then((found) => {
        if (!live) return;
        // A custom profile cannot exist before the first run: the presets are the styles.
        setProfiles(found.profiles.filter((profile) => profile.builtin));
        setActive(found.active);
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  // Moving to another step puts focus on its heading, so a screen reader announces it.
  useEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      return;
    }
    heading.current?.focus({ preventScroll: true });
  }, [step]);

  useEffect(() => {
    entriesRef.current = entries;
  }, [entries]);

  const patchEntry = useCallback((path: string, change: (entry: FolderEntry) => FolderEntry) => {
    setEntries((current) => current.map((entry) => (entry.path === path ? change(entry) : entry)));
  }, []);

  /** Looks inside a folder and fills in its row. */
  const analyse = useCallback(
    (path: string) => {
      void currentOnboardingBackend()
        .analyze(path)
        .then((found) => {
          patchEntry(path, () => readyEntry({ ...found, path }));
        })
        .catch((error: unknown) => {
          patchEntry(path, () => failedEntry(path, analysisError(path, error)));
        });
    },
    [patchEntry],
  );

  const addPaths = useCallback(
    (paths: readonly string[]) => {
      const fresh: string[] = [];
      for (const path of paths) {
        const known = [...entriesRef.current.map((entry) => entry.path), ...fresh];
        if (!known.some((other) => samePath(other, path))) fresh.push(path);
      }
      if (fresh.length === 0) return;
      entriesRef.current = [...entriesRef.current, ...fresh.map(lookingEntry)];
      setEntries(entriesRef.current);
      fresh.forEach(analyse);
    },
    [analyse],
  );

  // A browser preview starts with sample folders, so each screen can be looked at.
  useEffect(() => {
    start?.folders.forEach(analyse);
  }, [start, analyse]);

  const over = useFolderDrop(addPaths);

  // One folder picker at a time: clicks while it is open are ignored.
  const picking = useRef(false);
  const browse = () => {
    if (picking.current) return;
    picking.current = true;
    void currentOnboardingBackend()
      .pickFolders()
      .then((paths) => {
        if (paths.length > 0) addPaths(paths);
      })
      .catch(() => undefined)
      .finally(() => {
        picking.current = false;
      });
  };

  const setKind = (path: string, kind: SourceKind) => {
    patchEntry(path, (entry) => ({ ...entry, kind, error: null }));
  };

  const remove = (path: string) => {
    setEntries((current) => current.filter((entry) => entry.path !== path));
  };

  const selected = chosen ?? active ?? profiles[0]?.id ?? "clean";
  // The example is the first video the folder scan found, in the order of the list.
  const exampleFile =
    foldersToAdd(entries).find((entry) => entry.kind === "movie" && entry.found?.firstVideo != null)
      ?.found?.firstVideo ?? null;
  const tmdbWorks = keys.providers.find((provider) => provider.id === "tmdb")?.state === "active";

  /** Adds the folders; returns false when one was refused (its row says why). */
  const addFolders = async (): Promise<boolean> => {
    const sources = currentSourcesBackend();
    let ok = true;
    for (const entry of foldersToAdd(entries)) {
      if (entry.added) continue;
      try {
        await sources.add(entry.kind, entry.path);
        patchEntry(entry.path, (current) => ({ ...current, added: true, error: null }));
      } catch (error) {
        ok = false;
        // A refused folder cannot be added as it is: its row says why and the user removes it.
        patchEntry(entry.path, (current) => ({
          ...current,
          status: "failed",
          error: t("setup.buildFailed", { reason: messageOf(error) }),
        }));
      }
    }
    return ok;
  };

  /**
   * Leaves the setup. Finishing adds the folders and makes the style active; skipping leaves
   * either out. In every case the tools download in the background and the setup is not shown
   * again.
   */
  const finish = async ({ folders, style }: FinishOptions) => {
    if (building) return;
    setBuilding(true);
    try {
      const adding = folders ? foldersToAdd(entries) : [];
      if (folders && !(await addFolders())) {
        // Back to the folders, where each refused one says why.
        setStep(1);
        return;
      }
      if (style && chosen !== null && chosen !== active) {
        try {
          await currentOrganizeBackend().selectProfile(chosen);
        } catch (error) {
          showToast({ tone: "error", message: messageOf(error) });
        }
      }
      useSettingsStore.getState().set(ONBOARDING_DONE_KEY, true);
      // A new install has nothing to catch up on: the version it starts with counts as seen.
      await markCurrentVersionSeen();
      await settleSettingSaves();
      // In the background: the scan reads the video details as soon as MediaInfo is there.
      const queued = await queueRecommendedTools();
      void downloadTools(queued);
      if (adding.length > 0) await runRescan();
      const kinds = adding.map((entry) => entry.kind);
      onFinished(kinds.includes("movie") || !kinds.includes("tv") ? "movies" : "tv");
    } finally {
      setBuilding(false);
    }
  };

  const skipStep = () => {
    if (step === 1) setEntries([]);
    if (step < LAST_STEP) setStep((step + 1) as Step);
    else void finish({ folders: true, style: false });
  };

  const cont = () => {
    if (step < LAST_STEP) setStep((step + 1) as Step);
    else void finish({ folders: true, style: true });
  };

  const reason =
    step === 1 && !readyToContinue(entries)
      ? folderStepReason(entries)
      : step === 2 && !tmdbWorks
        ? t("setup.tmdb.needsKey")
        : undefined;

  return (
    <div className="flex h-full flex-col overflow-hidden bg-surface-window text-text">
      <header
        data-tauri-drag-region
        className={cx(
          "flex h-[38px] shrink-0 items-center select-none",
          platform === "macos" ? "pl-20" : "pl-4",
        )}
      >
        <div className="pointer-events-none flex items-center">
          <BrandMark size={16} />
          <span className="sr-only">CineVault</span>
        </div>
        <div data-tauri-drag-region className="h-full min-w-0 flex-1" />
        {platform === "windows" || platform === "linux" ? (
          <CaptionButtons windowApi={api} maximized={maximized} />
        ) : null}
      </header>
      <div data-testid="onboarding-scroll" className="flex min-h-0 flex-1 overflow-y-auto">
        <main className="m-auto flex w-[728px] max-w-full flex-col gap-[22px] px-6 py-6">
          <Stepper step={step} />
          <section
            aria-label={t("setup.region")}
            className="overflow-hidden rounded-(--r-14) border border-solid border-line-edge bg-surface-floating shadow-floating"
          >
            {step === 1 ? (
              <FoldersStep
                entries={entries}
                over={over}
                headingRef={heading}
                onBrowse={browse}
                onKind={setKind}
                onRemove={remove}
              />
            ) : null}
            {step === 2 ? <TmdbStep keys={keys} headingRef={heading} /> : null}
            {step === 3 ? <RatingsStep keys={keys} headingRef={heading} /> : null}
            {step === 4 ? (
              <NamingStep
                profiles={profiles}
                selected={selected}
                onSelect={setChosen}
                fileName={exampleFile}
                headingRef={heading}
              />
            ) : null}
            <div className="flex items-center gap-2 border-t border-solid border-line-section bg-surface-floating px-4 py-3.5">
              <Button
                variant="ghost"
                disabled={building}
                onClick={() => {
                  void finish({ folders: false, style: false });
                }}
              >
                {t("setup.skipSetup")}
              </Button>
              <span className="flex-1" />
              <Button variant="ghost" disabled={building} onClick={skipStep}>
                {t("setup.skipStep")}
              </Button>
              <Button
                variant="primary"
                disabled={building}
                {...(reason === undefined
                  ? {}
                  : {
                      disabledReason: reason,
                      enabledBy:
                        step === 2 ? t("setup.tmdb.needsKeyFix") : t("setup.folders.needOneFix"),
                    })}
                onClick={cont}
              >
                {building ? t("setup.building") : t("setup.continue")}
              </Button>
            </div>
          </section>
          <p className="m-0 text-center text-(length:--fs-12) text-text-muted">
            {t("setup.laterNote")}
          </p>
        </main>
      </div>
    </div>
  );
}
