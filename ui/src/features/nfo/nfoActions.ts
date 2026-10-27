import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentNfoBackend } from "./nfoBackend";
import type { NfoActionResult, NfoScope } from "./nfoBackend";

export type NfoActionKind = "read" | "rewrite";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The library views show what the files hold (or the other way round) once they refetch. */
function refreshLibrary(): void {
  void queryClient.invalidateQueries();
}

/** One line per item that was left out, in the words of its note code. */
function noteLines(result: NfoActionResult): string {
  const lines = result.notes.map((note) => {
    const reason = t(`nfo.note.${note.code}` as MessageKey);
    const detail = note.detail === "" ? "" : ` (${note.detail})`;
    return `${note.label}: ${reason}${detail}`;
  });
  const hidden = result.skipped + result.failed + result.kept_foreign - result.notes.length;
  if (hidden > 0) lines.push(`+${hidden}`);
  return lines.join("\n");
}

/** The toast text: what changed, or that nothing needed to. */
function summary(kind: NfoActionKind, result: NfoActionResult): string {
  if (result.changed === 0) {
    return t(kind === "read" ? "nfo.toast.nothingRead" : "nfo.toast.nothingRewritten");
  }
  return t(kind === "read" ? "nfo.toast.read" : "nfo.toast.rewrite", { count: result.changed });
}

async function undoAction(token: string): Promise<void> {
  const result = await currentNfoBackend().undo(token);
  refreshLibrary();
  showToast({ message: t("nfo.toast.undone", { count: result.restored }) });
  if (result.skipped.length > 0) {
    notify({
      severity: "warning",
      title: t("nfo.undo.skipped"),
      detail: result.skipped.join("\n"),
    });
  }
}

/**
 * Runs Read NFO or Rewrite NFO on a scope and reports it: an Undo toast for what changed, and a
 * notification for titles that were left out (a file that CineVault did not write is never
 * touched, and is listed there). A failure goes to the notification centre, never to a dialog.
 */
export async function runNfoAction(kind: NfoActionKind, scope: NfoScope): Promise<void> {
  const backend = currentNfoBackend();
  let result: NfoActionResult;
  try {
    result = kind === "read" ? await backend.read(scope) : await backend.rewrite(scope);
  } catch (error) {
    notify({
      severity: "error",
      title: t("nfo.notice.failed", { message: messageOf(error) }),
    });
    return;
  }
  if (result.changed > 0 || result.files_trashed > 0) refreshLibrary();

  const token = result.token;
  showToast({
    message: summary(kind, result),
    ...(token === null
      ? {}
      : {
          undo: async () => {
            await undoAction(token);
          },
        }),
  });

  if (result.skipped + result.failed + result.kept_foreign > 0) {
    notify({
      severity: result.failed > 0 ? "error" : "warning",
      title: t(kind === "read" ? "nfo.notice.read" : "nfo.notice.rewrite"),
      detail: noteLines(result),
      source: "NFO",
    });
  }
}

/** The scope of the selected items, as the library views hold their ids. */
export function selectionScope(ids: readonly string[]): NfoScope {
  return { kind: "selection", ids: [...ids] };
}
