import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Takes titles out of the library and leaves their files: "Remove from library (keep files)", or
 * with `ignore` also "Ignore this folder", which marks the folder so scans skip it. Says what
 * happened with a plain toast, or a notification when it failed. Resolves to true when the titles
 * were removed. `label` names the title, or "4 titles", in the messages.
 */
export async function removeTitlesFromLibrary(
  ids: readonly string[],
  ignore: boolean,
  label: string,
): Promise<boolean> {
  const fail = (detail: string) =>
    notify({ severity: "error", title: t("title.remove.failed", { title: label }), detail });
  try {
    const report = await currentLibraryOpsBackend().removeFromLibrary(ids, ignore);
    const problem = report.problems[0];
    if (problem !== undefined || report.removed.length === 0) {
      fail(problem === undefined ? "" : problem.reason);
      return false;
    }
    const skipped = report.removed.some((entry) => entry.ignore === "skipped");
    showToast({
      message: skipped
        ? t("title.ignore.skipped", { title: label })
        : t(ignore ? "title.ignore.done" : "title.remove.done", { title: label }),
    });
    // The listings, counts and sidebar refetch what the removal changed.
    void queryClient.invalidateQueries();
    return true;
  } catch (error) {
    fail(messageOf(error));
    return false;
  }
}
