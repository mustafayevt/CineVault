import { FolderPen } from "lucide-react";
import { create } from "zustand";

import type { QueryClient } from "@tanstack/react-query";

import type { AppliedDto, OrganizeRequestDto, UndoOutcomeDto } from "../../bindings";
import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { openActivityAction } from "../activity/openActivity";
import { undoBatchInHistory } from "../history/historyUndo";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentOrganizeBackend } from "./organizeBackend";

/** Query key of everything the Rename page reads; anything that renames or reads details invalidates it. */
export const RENAME_KEY = ["rename"] as const;

/** The notice action that opens the titles a rename could not rename in the Rename page. */
export const OPEN_RENAME_ACTION = "open_rename";

/** A rename that is running: what the sidebar's progress card shows. */
export interface RunningRename {
  /** "Renaming 12 titles". */
  name: string;
  done: number;
  total: number;
  /** The title that just finished; empty before the first. */
  title: string;
}

interface RenameRunState {
  running: RunningRename | null;
}

/** The rename in progress, if any; it outlives the Rename page. */
export const useRenameRunStore = create<RenameRunState>(() => ({ running: null }));

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Everything that shows names or stages, read again after a rename or an undo. */
export async function refreshAfterRename(client: QueryClient = queryClient): Promise<void> {
  await Promise.all([
    client.invalidateQueries({ queryKey: RENAME_KEY }),
    client.invalidateQueries({ queryKey: ["lifecycle"] }),
    client.invalidateQueries({ queryKey: ["library"] }),
    client.invalidateQueries({ queryKey: ["movie-detail"] }),
    client.invalidateQueries({ queryKey: ["tv-show"] }),
  ]);
}

/** What the toast of an undo says: how many titles came back, and how many did not. */
function undoMessage(outcome: UndoOutcomeDto): string {
  const left = outcome.not_restorable.length + outcome.failed.length;
  if (outcome.restored === 0 && left > 0) return t("renamePage.undo.nothing");
  const done = t("renamePage.undo.done", { count: outcome.restored });
  return left === 0
    ? done
    : t("renamePage.undo.partly", { done, left: t("renamePage.undo.left", { count: left }) });
}

/**
 * Takes a rename back, through its History entry when it has one, else through its journal, and
 * says how it went. Also the Undo of a rename that ran by itself (Rename after Scrape).
 */
export async function undoRename(batchId: string, client: QueryClient): Promise<void> {
  const report = await undoBatchInHistory(batchId);
  const outcome =
    report === null
      ? await currentOrganizeBackend().undo(batchId)
      : {
          batch_id: batchId,
          restored: report.undone.length,
          not_restorable: report.left.map((entry) => ({
            title: entry.title,
            path: "",
            reason: entry.reason,
          })),
          failed: [],
        };
  showToast({ message: undoMessage(outcome) });
  await refreshAfterRename(client);
}

/** Opens the Rename page with the preview of these titles. */
export function openRename(ids: readonly string[]): void {
  void import("../../router").then(({ router }) =>
    router.navigate({ to: "/rename", search: { ids: ids.join(",") } as never }),
  );
}

/**
 * The toast and the Needs attention entry of a finished rename (spec `renamer` "Rename page
 * footer and result"): "Renamed 12 titles" with Undo; with failures "Renamed 10 of 12 titles;
 * 2 need attention" with Undo and Open Activity, and the failed titles in Needs attention with
 * their reasons and "Open in Rename".
 */
export function reportRename(applied: AppliedDto, client: QueryClient = queryClient): void {
  const batchId = applied.batch_id;
  const failures = applied.items.filter((item) => item.status === "failed");
  if (failures.length > 0) {
    notify({
      severity: "error",
      title: t("renamePage.notice.failed", { count: failures.length }),
      detail: failures.map((item) => `${item.title}: ${item.error}`).join("\n"),
      source: t("renamePage.region"),
      action: OPEN_RENAME_ACTION,
      itemIds: failures.map((item) => item.id),
    });
  }
  if (batchId === null) {
    showToast({ message: t("renamePage.toast.nothing") });
    return;
  }
  const total = applied.done + applied.failed;
  if (applied.done === 0 && applied.failed > 0) {
    showToast({
      message: t("renamePage.toast.noneDone", { count: applied.failed }),
      tone: "error",
      action: openActivityAction(),
    });
    return;
  }
  showToast({
    glyph: FolderPen,
    message:
      applied.failed === 0
        ? t("renamePage.toast.done", { count: applied.done })
        : t("renamePage.toast.partly", { done: applied.done, total, count: applied.failed }),
    undo: () => undoRename(batchId, client),
    ...(applied.failed === 0 ? {} : { action: openActivityAction() }),
  });
}

/**
 * Renames the titles with these ids. It runs outside any page: while it runs the sidebar's
 * progress card shows "Renaming 3 of 12", and its toast appears wherever the user is when it
 * ends. Only one rename runs at a time; resolves `false` when one already was. `client` is the
 * app's query client, read again when it ends.
 */
export async function runRename(
  request: OrganizeRequestDto,
  ids: readonly string[],
  client: QueryClient = queryClient,
): Promise<boolean> {
  const store = useRenameRunStore;
  if (store.getState().running !== null || ids.length === 0) return false;
  const name = t("renamePage.progress.name", { count: ids.length });
  store.setState({ running: { name, done: 0, total: ids.length, title: "" } });
  try {
    const applied = await currentOrganizeBackend().apply(request, ids, (progress) => {
      store.setState({
        running: {
          name,
          done: progress.done,
          total: progress.total,
          title: progress.title,
        },
      });
    });
    reportRename(applied, client);
  } catch (error) {
    notify({
      severity: "error",
      title: t("renamePage.toast.couldNot"),
      detail: messageOf(error),
    });
  } finally {
    store.setState({ running: null });
    await refreshAfterRename(client);
  }
  return true;
}
