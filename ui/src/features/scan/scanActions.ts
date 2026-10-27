import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentScanBackend } from "./scanBackend";
import type { MissingInSource } from "./scanBackend";

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
