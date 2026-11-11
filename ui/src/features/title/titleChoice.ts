import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { currentEditBackend } from "../edit/editBackend";
import type { TitleChoice } from "../edit/editBackend";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The listings, the title pages and the Edit page read the changed titles again. */
function refreshTitles(): void {
  void queryClient.invalidateQueries();
}

function warnAboutNfo(report: TitleChoice): void {
  if (report.nfo_problem !== null) {
    notify({ severity: "warning", title: t("title.choice.nfoFailed"), detail: report.nfo_problem });
  }
}

/**
 * Uses the original title (`original`) or the localized title for the chosen movies and shows:
 * "Use original title" and "Use localized title" under the selection bar's More and in the
 * right-click menu. The library, the file and folder names (through the usual Rename pending)
 * and the NFO follow. Confirms with a toast that offers Undo, which switches back exactly the
 * titles that changed. `subject` names the title, or "4 titles". Resolves to true when a title
 * changed.
 */
export async function setTitleChoice(
  ids: readonly string[],
  original: boolean,
  subject: string,
): Promise<boolean> {
  const backend = currentEditBackend();
  try {
    const report = await backend.setTitleChoice(ids, original);
    refreshTitles();
    if (report.changed.length === 0) {
      showToast({ message: t("title.choice.nothing") });
      return false;
    }
    const named =
      report.unchanged === 0 ? subject : t("library.act.subject", { count: report.changed.length });
    const done = t(original ? "title.choice.original.done" : "title.choice.localized.done", {
      subject: named,
    });
    showToast({
      message:
        report.unchanged === 0
          ? done
          : `${done}. ${t("title.choice.someUnchanged", { count: report.unchanged })}`,
      undo: async () => {
        warnAboutNfo(await backend.setTitleChoice(report.changed, !original));
        refreshTitles();
      },
    });
    warnAboutNfo(report);
    return true;
  } catch (error) {
    notify({
      severity: "error",
      title: t("title.choice.failed", { subject }),
      detail: messageOf(error),
    });
    return false;
  }
}

/**
 * "Use for titles I have" beside Settings › Keep the original title: every movie and show already
 * in the library whose original language is listed starts using its original title, with Undo.
 */
export async function applyOriginalTitleLanguages(): Promise<void> {
  const backend = currentEditBackend();
  try {
    const report = await backend.applyOriginalTitleLanguages();
    refreshTitles();
    showToast({
      message: t("settingsPage.providers.originalTitles.applied", {
        count: report.changed.length,
      }),
      ...(report.changed.length === 0
        ? {}
        : {
            undo: async () => {
              warnAboutNfo(await backend.setTitleChoice(report.changed, false));
              refreshTitles();
            },
          }),
    });
    warnAboutNfo(report);
  } catch (error) {
    notify({
      severity: "error",
      title: t("settingsPage.providers.originalTitles.failed"),
      detail: messageOf(error),
    });
  }
}
