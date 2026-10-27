import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentMediaInfoBackend } from "./mediaInfoBackend";
import type { ReloadResult } from "./mediaInfoBackend";

interface ReloadState {
  /** A reload is running; another cannot start until it ends. */
  running: boolean;
}

export const useReloadStore = create<ReloadState>(() => ({ running: false }));

/** The lines of the titles that could not be read again. */
function failureLines(result: ReloadResult): string {
  const lines = result.lines.map((line) =>
    line.message === null ? line.title : `${line.title}: ${line.message}`,
  );
  const hidden = result.failed - lines.length;
  if (hidden > 0) lines.push(`+${hidden}`);
  return lines.join("\n");
}

/**
 * Reload media info for these titles: their stored file details are forgotten and read from the
 * files again as a background job (the sidebar progress card shows it and can cancel it). A toast
 * says how it ended; titles that could not be read go to the notification centre, and a missing
 * MediaInfo or a failure is reported there too, never in a dialog.
 */
export async function reloadMediaInfo(ids: readonly string[]): Promise<void> {
  if (ids.length === 0 || useReloadStore.getState().running) return;
  useReloadStore.setState({ running: true });
  showToast({ message: t("library.reloadMedia.started", { count: ids.length }) });
  let result: ReloadResult;
  try {
    result = await currentMediaInfoBackend().reload(ids);
  } catch (error) {
    notify({
      severity: "error",
      title: t("library.reloadMedia.failed", {
        message: error instanceof Error ? error.message : String(error),
      }),
      source: t("library.reloadMedia.source"),
    });
    return;
  } finally {
    useReloadStore.setState({ running: false });
  }
  // The table, the grid and the open title page show the new details.
  void queryClient.invalidateQueries();
  if (result.status === "unavailable" || result.status === "failed") {
    notify({
      severity: result.status === "failed" ? "error" : "warning",
      title: result.message,
      source: t("library.reloadMedia.source"),
    });
    return;
  }
  showToast({ message: result.message });
  if (result.failed > 0) {
    notify({
      severity: "warning",
      title: t("library.reloadMedia.someFailed", { count: result.failed }),
      detail: failureLines(result),
      source: t("library.reloadMedia.source"),
    });
  }
}
