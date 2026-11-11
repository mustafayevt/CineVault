import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { whenJobEnds } from "../jobs/jobProgressStore";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentScanBackend } from "./scanBackend";
import type { MissingInSource, RescanStarted } from "./scanBackend";

interface ScanUiState {
  /** True from pressing Rescan until the backend has queued the job. */
  starting: boolean;
  /** True while Find missing is reading the library folders. */
  finding: boolean;
  /** What the last Find missing found; `null` before the first search. */
  missing: readonly MissingInSource[] | null;
  /** The sentence to show when the last search failed. */
  findError: string | null;
  /** Bumped by the "Find missing files" command; the library folders page runs the search. */
  findRequest: number;
  requestFindMissing: () => void;
}

export const useScanUiStore = create<ScanUiState>((set) => ({
  starting: false,
  finding: false,
  missing: null,
  findError: null,
  findRequest: 0,
  requestFindMissing: () => set((state) => ({ findRequest: state.findRequest + 1 })),
}));

/**
 * Rescans every library folder. A toast confirms that the scan started; the job itself shows in
 * the activity pill and drawer. A failure goes to the notification centre. The backend tells the
 * user, once, when MediaInfo is missing and media details stay pending.
 */
export async function runRescan(): Promise<void> {
  if (useScanUiStore.getState().starting) return;
  const backend = currentScanBackend();
  if (!backend.available) {
    showToast({ message: t("scan.desktopOnly") });
    return;
  }
  useScanUiStore.setState({ starting: true });
  try {
    const started = await backend.rescan();
    if (started.sources === 0) {
      showToast({ message: t("scan.noFolders") });
    } else {
      showToast({ message: t("scan.started", { count: started.sources }) });
    }
  } catch (error) {
    notify({
      severity: "error",
      title: t("scan.couldNotStart"),
      detail: error instanceof Error ? error.message : String(error),
      source: t("scan.heading"),
    });
  } finally {
    useScanUiStore.setState({ starting: false });
  }
}

/** The one-place scans being queued, by what they scan, so a double click queues one. */
const queuing = new Set<string>();

/**
 * Queues a scan of one place and confirms it with "Scanning <name>"; the job then shows on the
 * progress card and in Activity. A refusal goes to Activity. Resolves with the queued job, or
 * `null` when nothing was queued.
 */
async function scanOne(
  key: string,
  name: string,
  queue: () => Promise<RescanStarted>,
): Promise<RescanStarted | null> {
  if (queuing.has(key)) return null;
  const backend = currentScanBackend();
  if (!backend.available) {
    showToast({ message: t("scan.desktopOnly") });
    return null;
  }
  queuing.add(key);
  try {
    const started = await queue();
    showToast({ message: t("scan.startedOne", { name }) });
    return started;
  } catch (error) {
    notify({
      severity: "error",
      title: t("scan.couldNotStart"),
      detail: error instanceof Error ? error.message : String(error),
      source: t("scan.heading"),
    });
    return null;
  } finally {
    queuing.delete(key);
  }
}

/**
 * "Scan this folder" (spec `library-scanning` "Scan one library folder"): scans one library
 * folder, named by its label, and nothing else.
 */
export async function runScanFolder(sourceId: number, name: string): Promise<void> {
  await scanOne(`folder:${sourceId}`, name, () => currentScanBackend().scanFolder(sourceId));
}

/**
 * "Scan again" (spec `library-scanning` "Scan one title again"): scans the title's own folder.
 * When the scan ends the title's page is read again, so files that are back show at once.
 */
export async function runScanTitle(id: string, name: string): Promise<void> {
  const started = await scanOne(`title:${id}`, name, () => currentScanBackend().scanTitle(id));
  if (started === null || started.job_id === 0) return;
  await whenJobEnds(started.job_id);
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["movie-detail"] }),
    queryClient.invalidateQueries({ queryKey: ["tv-show"] }),
  ]);
}
