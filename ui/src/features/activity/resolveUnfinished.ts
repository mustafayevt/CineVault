import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { currentFormat } from "../../i18n/useFormat";
import { queryClient } from "../../queryClient";
import { HISTORY_KEY } from "../history/historyBackend";
import { messageOf } from "../library-ops/manageShared";
import { movePercent, moveSpeedText } from "../library-ops/moveModel";
import { refreshAfterMove, undoMove } from "../move-to/runMove";
import { useMoveToStore } from "../move-to/moveToStore";
import { notify, useNotificationStore } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { openActivityAction } from "./openActivity";
import { currentRecoveryBackend, UNFINISHED_KEY } from "./recoveryBackend";
import type { UnfinishedBatch } from "./recoveryBackend";

/** What is being done to an unfinished batch from this window right now. */
export type BatchWork = "resume" | "rollBack";

interface RecoveryState {
  /** The batch being resumed: the backend stops listing a batch while it runs. */
  active: UnfinishedBatch | null;
  /** Batches with a Resume or Roll back under way, so their buttons are not pressed twice. */
  working: Readonly<Record<string, BatchWork>>;
  setActive: (batch: UnfinishedBatch | null) => void;
  setWorking: (batchId: string, work: BatchWork | null) => void;
}

export const useRecoveryStore = create<RecoveryState>((set) => ({
  active: null,
  working: {},
  setActive: (active) => set({ active }),
  setWorking: (batchId, work) =>
    set((state) => ({
      working: Object.fromEntries([
        ...Object.entries(state.working).filter(([id]) => id !== batchId),
        ...(work === null ? [] : [[batchId, work] as const]),
      ]),
    })),
}));

/** The titles of a batch in a few words: "Heat", "Heat and 2 more" or "3 titles". */
export function batchLabel(batch: UnfinishedBatch): string {
  const [first] = batch.titles;
  if (first === undefined) return t("move.titles", { count: Math.max(1, batch.items) });
  if (batch.items <= 1) return first;
  return t("activity.batch.titles", { first, count: batch.items - 1 });
}

/** How far the batch got: "8 of 10 files copied, 15 GB of 86 GB" or "2 of 5 titles renamed". */
export function batchProgress(batch: UnfinishedBatch, size: (bytes: number) => string): string {
  const counts = { done: batch.files_done, total: batch.files_total };
  if (batch.kind === "rename") return t("activity.batch.renamed", counts);
  if (batch.bytes_total === null || batch.bytes_total <= 0) {
    return t("activity.batch.files", counts);
  }
  return t("activity.batch.filesBytes", {
    ...counts,
    bytesDone: size(batch.bytes_done ?? 0),
    bytesTotal: size(batch.bytes_total),
  });
}

/** Everything a resume or a roll back can change is read again. */
async function refreshAfterRecovery(): Promise<void> {
  await Promise.all([
    refreshAfterMove(),
    queryClient.invalidateQueries({ queryKey: HISTORY_KEY }),
    // The Move to menu's plans: a title it blocked can be moved again.
    queryClient.invalidateQueries({ queryKey: ["move-to"] }),
  ]);
}

/**
 * Carries an unfinished batch on (spec `drives` "Unfinished moves are recovered"). Its progress
 * shows in the sidebar's progress card and in its Needs attention row, with Pause and Cancel; the
 * end is said in a toast, with Undo when a move finished. Only one move runs at a time: returns
 * false without starting when another one is running.
 */
export async function resumeBatch(batch: UnfinishedBatch): Promise<boolean> {
  const recovery = useRecoveryStore.getState();
  if (recovery.working[batch.batch_id] !== undefined) return false;
  if (useMoveToStore.getState().running !== null) {
    showToast({ message: t("activity.batch.busy") });
    return false;
  }
  const label = batchLabel(batch);
  const target = batch.destination ?? "";
  const move = batch.kind === "move";
  const running = { label, target, batchId: batch.batch_id };
  recovery.setWorking(batch.batch_id, "resume");
  recovery.setActive(batch);
  if (move) useMoveToStore.getState().setRunning({ ...running, percent: 0, paused: false });
  try {
    const outcome = await currentRecoveryBackend().resume(batch.batch_id, (progress) => {
      if (!move) return;
      const speed = moveSpeedText(progress, (bytes) => currentFormat().size(bytes));
      useMoveToStore.getState().setRunning({
        ...running,
        percent: movePercent(progress),
        current: progress.title,
        ...(speed === null ? {} : { speed }),
        paused: useMoveToStore.getState().running?.paused ?? false,
      });
    });
    useNotificationStore.getState().resolveBatch(batch.batch_id);
    if (outcome.stopped === null) {
      showToast({
        message: move ? t("activity.toast.resumed", { label }) : t("activity.toast.resumedRename"),
        ...(move ? { undo: () => undoMove(batch.batch_id, label, target) } : {}),
      });
    } else {
      // Still unfinished: its row in Needs attention says why, with Resume and Roll back.
      showToast({
        message: t("activity.toast.stoppedAgain", { label, reason: outcome.stopped }),
        tone: "error",
        action: openActivityAction(batch.batch_id),
      });
    }
    if (outcome.notes.length > 0) {
      notify({
        severity: outcome.stopped === null ? "info" : "warning",
        title: t("activity.notice.resumeNotes"),
        detail: outcome.notes.join("\n"),
        source: label,
      });
    }
    return true;
  } catch (error) {
    notify({
      severity: "error",
      title: t("activity.notice.resumeFailed"),
      detail: messageOf(error),
      source: label,
    });
    return true;
  } finally {
    if (move) useMoveToStore.getState().setRunning(null);
    const state = useRecoveryStore.getState();
    if (state.active?.batch_id === batch.batch_id) state.setActive(null);
    state.setWorking(batch.batch_id, null);
    await refreshAfterRecovery();
  }
}

/**
 * Takes an unfinished batch back: only what the batch made is removed, and the originals were
 * never touched. Says so in a toast; whatever could not be taken back waits in Needs attention.
 */
export async function rollBackBatch(batch: UnfinishedBatch): Promise<boolean> {
  const recovery = useRecoveryStore.getState();
  if (recovery.working[batch.batch_id] !== undefined) return false;
  const label = batchLabel(batch);
  recovery.setWorking(batch.batch_id, "rollBack");
  try {
    const outcome = await currentRecoveryBackend().rollBack(batch.batch_id);
    useNotificationStore.getState().resolveBatch(batch.batch_id);
    showToast({
      message:
        batch.kind === "move"
          ? t("activity.toast.rolledBack", { label })
          : t("activity.toast.rolledBackRename"),
    });
    if (outcome.problems.length > 0) {
      notify({
        severity: "warning",
        title: t("activity.notice.rollBackProblems", { count: outcome.problems.length }),
        detail: outcome.problems.join("\n"),
        source: label,
      });
    }
    return true;
  } catch (error) {
    notify({
      severity: "error",
      title: t("activity.notice.rollBackFailed"),
      detail: messageOf(error),
      source: label,
    });
    return false;
  } finally {
    useRecoveryStore.getState().setWorking(batch.batch_id, null);
    await refreshAfterRecovery();
  }
}

let autoResumed = false;

/** Lets the start-up resume run again. Tests only. */
export function resetAutoResume(): void {
  autoResumed = false;
}

/**
 * Once per window, at start-up: every move that the app's close or a crash cut short, with its
 * drives and folders all there, carries on by itself, one after the other, from where it stopped
 * (a half-copied file continues). A toast says so and leads to Activity, where Pause, Cancel and
 * Roll back are. A move that stopped on an error waits in Needs attention for the user.
 */
export async function autoResumeUnfinished(): Promise<void> {
  if (autoResumed) return;
  autoResumed = true;
  let batches: UnfinishedBatch[];
  try {
    batches = await queryClient.fetchQuery({
      queryKey: UNFINISHED_KEY,
      queryFn: () => currentRecoveryBackend().list(),
      staleTime: 0,
    });
  } catch {
    // Needs attention reads the list again and says what it can.
    return;
  }
  for (const batch of batches.filter((entry) => entry.auto_resume)) {
    if (useMoveToStore.getState().running !== null) return;
    showToast({
      message: t("activity.toast.resuming", { label: batchLabel(batch) }),
      action: openActivityAction(batch.batch_id),
    });
    await resumeBatch(batch);
  }
}
