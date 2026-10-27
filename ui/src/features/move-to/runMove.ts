import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { DRIVES_KEY } from "../drives/drivesBackend";
import { undoBatchInHistory } from "../history/historyUndo";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import type { MoveApplied } from "../library-ops/libraryOpsBackend";
import { messageOf, refreshLibrary } from "../library-ops/manageShared";
import { currentFormat } from "../../i18n/useFormat";
import { movePercent, moveSpeedText } from "../library-ops/moveModel";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import type { MoveOption } from "./moveOptions";
import { useMoveToStore } from "./moveToStore";

/** The library and the drives read again after a move or its Undo. */
async function refreshAfterMove(): Promise<void> {
  await Promise.all([
    refreshLibrary(queryClient),
    queryClient.invalidateQueries({ queryKey: DRIVES_KEY }),
  ]);
}

/** Takes a finished move back from its Undo toast and says how it went. */
async function undoMove(batch: string, what: string, target: string): Promise<void> {
  const backend = currentLibraryOpsBackend();
  try {
    // The move's History entry is the record: undoing here or from History is the same Undo.
    const report = await undoBatchInHistory(batch);
    const outcome =
      report === null
        ? await backend.undoBatch(batch)
        : {
            restored: report.undone.length,
            not_restorable: report.left.map((entry) => ({
              title: entry.title,
              path: "",
              reason: entry.reason,
            })),
            failed: [],
          };
    await refreshAfterMove();
    if (outcome.restored > 0) showToast({ message: t("move.toast.undone", { label: what }) });
    else showToast({ message: t("move.toast.notUndone", { label: what, target }) });
    const left = outcome.not_restorable.length + outcome.failed.length;
    if (left > 0) {
      notify({
        severity: "warning",
        title: t("manage.move.undoPartial", { count: left }),
        detail: [...outcome.not_restorable, ...outcome.failed]
          .map((entry) => `${entry.title}: ${entry.reason}`)
          .join("\n"),
      });
    }
  } catch (error) {
    notify({ severity: "error", title: t("manage.move.undoFailed"), detail: messageOf(error) });
  }
}

/** The toast, notices and refresh that follow a finished move. */
function report(applied: MoveApplied, how: { label: string; target: string; many: boolean }): void {
  const batch = applied.batch_id;
  const stopped = applied.items.filter((item) => item.result === "stopped");
  const failed = applied.items.filter((item) => item.result === "failed");
  if (applied.moved > 0) {
    const what = how.many ? t("move.titles", { count: applied.moved }) : how.label;
    showToast({
      message: how.many
        ? t("manage.move.done", { count: applied.moved, target: how.target })
        : t("move.toast.done", { label: how.label, target: how.target }),
      ...(batch === null ? {} : { undo: () => undoMove(batch, what, how.target) }),
    });
    for (const item of applied.items) {
      for (const note of item.notes) notify({ severity: "info", title: note });
    }
  } else if (applied.items.some((item) => item.result === "canceled")) {
    showToast({ message: t("move.toast.cancelled", { label: how.label }) });
  }
  if (stopped.length > 0) {
    notify({
      severity: "warning",
      title: t("move.toast.stopped", { label: how.label }),
      detail: stopped.map((item) => `${item.title}: ${item.reason}`).join("\n"),
      ...(batch === null ? {} : { batchId: batch, action: "resolve_unfinished" }),
    });
  }
  if (failed.length > 0) {
    notify({
      severity: "error",
      title: t("manage.move.notMoved", { count: failed.length }),
      detail: failed.map((item) => `${item.title}: ${item.reason}`).join("\n"),
    });
  }
}

/**
 * Starts the move of a destination's titles (spec `source-moves`): the backend copies, checks,
 * then sends the originals to the Recycle Bin. The Drives page shows the progress while it runs
 * and a toast offers Undo when it ends. Only one move runs at a time.
 */
export async function moveToDestination(option: MoveOption, label: string): Promise<void> {
  const store = useMoveToStore.getState();
  if (store.running !== null || option.readyIds.length === 0) return;
  const many = option.readyIds.length > 1;
  const what = many ? t("move.titles", { count: option.readyIds.length }) : label;
  const running = { label: what, target: option.name };
  store.setRunning({ ...running, percent: 0, paused: false });
  try {
    const applied = await currentLibraryOpsBackend().applyMove(
      option.readyIds,
      option.source.id,
      (progress) => {
        const speed = moveSpeedText(progress, (bytes) => currentFormat().size(bytes));
        useMoveToStore.getState().setRunning({
          ...running,
          percent: movePercent(progress),
          current: progress.title,
          ...(speed === null ? {} : { speed }),
          paused: useMoveToStore.getState().running?.paused ?? false,
        });
      },
    );
    await refreshAfterMove();
    report(applied, { label, target: option.name, many });
  } catch (error) {
    notify({ severity: "error", title: t("manage.move.failed"), detail: messageOf(error) });
  } finally {
    useMoveToStore.getState().setRunning(null);
  }
}
