import type { QueryClient } from "@tanstack/react-query";

import { t } from "../../i18n/catalog";
import { isServer } from "../../lib/transport";
import { refreshLibrary } from "../library-ops/manageShared";
import { currentOnboardingBackend } from "../onboarding/onboardingBackend";
import { probeFolder } from "../pickers/folderPickerRequest";
import { runRescan } from "../scan/scanActions";
import { currentScanBackend } from "../scan/scanBackend";
import { SOURCE_HEALTH_KEY, SOURCE_LABELS_KEY } from "../scan/useSourceHealth";
import { showToast } from "../toasts/toastStore";
import { currentSourcesBackend } from "./sourcesBackend";
import type { SourceKind } from "./sourcesBackend";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Reads the folders, their state and everything that depends on them again. */
export async function reloadFolders(client: QueryClient): Promise<void> {
  await Promise.all([
    client.invalidateQueries({ queryKey: SOURCE_LABELS_KEY }),
    client.invalidateQueries({ queryKey: SOURCE_HEALTH_KEY }),
    refreshLibrary(client),
  ]);
}

/** Says that folders were added, and scans them, so their titles appear without another click. */
export async function foldersAdded(client: QueryClient, added: number): Promise<void> {
  if (added === 0) return;
  await reloadFolders(client);
  showToast({ message: t("settingsPage.folders.added", { count: added }) });
  if (currentScanBackend().available) void runRescan();
}

/**
 * Adds the folders at `paths` to the library, each one refused or added on its own, and reports.
 * `kind` is the library they go to; without it each takes the type it looks like (a folder with
 * both kinds starts as movies; the switch on its row changes it). In a browser a folder the
 * server cannot write to is refused. Returns how many were added.
 */
export async function addLibraryFolders(
  client: QueryClient,
  paths: readonly string[],
  kind: SourceKind | null,
): Promise<number> {
  let added = 0;
  for (const path of paths) {
    try {
      if (isServer()) {
        const probe = await probeFolder(path);
        if (!probe.writable) {
          showToast({
            tone: "error",
            message: probe.message ?? "This folder is not writable by the server.",
          });
          continue;
        }
      }
      let chosen = kind;
      if (chosen === null) {
        const found = await currentOnboardingBackend().analyze(path);
        chosen = found.suggested === "tv" ? "tv" : "movie";
      }
      await currentSourcesBackend().add(chosen, path);
      added += 1;
    } catch (error) {
      showToast({ tone: "error", message: messageOf(error) });
    }
  }
  await foldersAdded(client, added);
  return added;
}
