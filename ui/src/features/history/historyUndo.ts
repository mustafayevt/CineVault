import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { showToast } from "../toasts/toastStore";
import { currentHistoryBackend, HISTORY_KEY } from "./historyBackend";
import type { UndoReport } from "./historyBackend";

/** The titles an Undo left, as one sentence: "Heat (1995): Undo "Renamed 12 titles" first." */
export function leftText(report: UndoReport): string {
  const shown = report.left
    .slice(0, 2)
    .map((entry) => t("history.toast.left", { title: entry.title, reason: entry.reason }));
  const more = report.left.length - shown.length;
  if (more > 0) shown.push(t("history.toast.more", { count: more }));
  return shown.join(" ");
}

/** Everything that shows files, names, artwork or stages is read again after an Undo. */
function refreshAfterUndo(): Promise<unknown> {
  return queryClient.invalidateQueries();
}

/**
 * Says what an Undo did when it was not the whole entry: some titles taken back and the others
 * left with their reasons ("Undo "Renamed 12 titles" first."). A complete Undo needs no word of
 * its own. Returns true when something was taken back.
 */
function announceUndo(report: UndoReport): boolean {
  const done = report.undone.length;
  if (report.left.length === 0) return done > 0;
  const text = leftText(report);
  showToast({
    message:
      done === 0
        ? t("history.toast.nothing", { left: text })
        : t("history.toast.partial", { done, total: done + report.left.length, left: text }),
    duration: 9000,
  });
  return done > 0;
}

/**
 * Takes a History entry back: the Undo of every toast and of the History page, so undoing from
 * either has the same result and marks the same entry (spec `operation-history` "Undo toasts
 * point to the same record"). Titles that could not be taken back are named in a toast. Resolves
 * to the report; rejects when the entry could not be undone at all.
 */
export async function undoHistoryEntry(
  entryId: string,
  titleId?: string,
  options: { announce?: boolean } = {},
): Promise<UndoReport> {
  const backend = currentHistoryBackend();
  const report =
    titleId === undefined ? await backend.undo(entryId) : await backend.undoTitle(entryId, titleId);
  void queryClient.invalidateQueries({ queryKey: HISTORY_KEY });
  await refreshAfterUndo();
  if (options.announce !== false) announceUndo(report);
  return report;
}

/**
 * Takes a rename or move batch back through the History entry that records it, so the toast's
 * Undo and the History page do the same and the entry reads "Undone". `null` when no entry
 * records the batch (the caller then undoes the journal batch itself). The caller says how it
 * went; nothing is announced here.
 */
export async function undoBatchInHistory(batchId: string): Promise<UndoReport | null> {
  const entry = await currentHistoryBackend().entryForBatch(batchId);
  return entry === null ? null : undoHistoryEntry(entry, undefined, { announce: false });
}
