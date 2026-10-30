import { Trash2 } from "lucide-react";
import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { currentHistoryBackend, HISTORY_KEY } from "../history/historyBackend";
import type { DeleteResult, EraseResult } from "../history/historyBackend";
import { undoHistoryEntry } from "../history/historyUndo";
import { notify } from "../notifications/notificationStore";
import type { DeletedMode } from "../settings/DeletedFilesSection";
import { readSetting } from "../settings/settingsStore";
import { binName } from "../shell/binName";
import { detectPlatform } from "../shell/platform";
import { showToast } from "../toasts/toastStore";

/** A title to delete: its id and the name the messages use. */
export interface DeleteTarget {
  id: string;
  title: string;
}

/** What the permanent-delete dialog was asked for. */
export interface PermanentRequest {
  targets: readonly DeleteTarget[];
  /** Runs after the titles were deleted (by either button), for example to leave a title page. */
  onDone?: (() => void) | undefined;
}

interface DeleteState {
  /** Titles Delete has hidden from the lists while it works. */
  hidden: ReadonlySet<string>;
  /** The open permanent-delete dialog; `null` when closed. */
  request: PermanentRequest | null;
  /** The open Delete confirmation; `null` when closed. */
  confirm: PermanentRequest | null;
  hide: (ids: readonly string[]) => void;
  unhide: (ids: readonly string[]) => void;
  ask: (request: PermanentRequest) => void;
  close: () => void;
  askConfirm: (request: PermanentRequest) => void;
  closeConfirm: () => void;
}

export const useDeleteStore = create<DeleteState>((set) => ({
  hidden: new Set(),
  request: null,
  confirm: null,
  hide: (ids) => set((state) => ({ hidden: new Set([...state.hidden, ...ids]) })),
  unhide: (ids) =>
    set((state) => {
      const gone = new Set(ids);
      return { hidden: new Set([...state.hidden].filter((id) => !gone.has(id))) };
    }),
  ask: (request) => set({ request }),
  close: () => set({ request: null }),
  askConfirm: (confirm) => set({ confirm }),
  closeConfirm: () => set({ confirm: null }),
}));

/** The Settings › Deleted files choice in effect; an unknown value counts as the Recycle Bin. */
export function deletedMode(): DeletedMode {
  const mode = readSetting<string>("files.deleted-mode");
  return mode === "folder" || mode === "custom" || mode === "erase" ? mode : "bin";
}

/**
 * Delete as the person starts it (the title page button, the selection bar, the context menu, the
 * Delete key): asks first. When Deleted files is Delete permanently it opens the permanent-delete
 * confirmation with the typed DELETE, always. Otherwise it opens the Delete confirmation, unless
 * Settings › Deleted files › "Ask before deleting" (`delete.confirm`) is off, in which case the
 * titles go at once with an Undo toast.
 */
export function requestDelete(
  targets: readonly DeleteTarget[],
  options: { onDone?: (() => void) | undefined } = {},
): void {
  if (targets.length === 0) return;
  if (deletedMode() === "erase") {
    requestPermanentDelete(targets, options);
    return;
  }
  if (!readSetting<boolean>("delete.confirm")) {
    void deleteToBin(targets, options);
    return;
  }
  useDeleteStore.getState().askConfirm({ targets, onDone: options.onDone });
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** "Heat (1995)" for one title, "4 titles" for several. */
function subjectOf(targets: readonly DeleteTarget[]): string {
  const [only] = targets;
  return targets.length === 1 && only !== undefined
    ? only.title
    : t("library.act.subject", { count: targets.length });
}

/** The listings, counts, sidebar and drives read what Delete changed. */
async function refreshAll(): Promise<void> {
  await queryClient.invalidateQueries();
}

function toastFor(result: DeleteResult): string {
  const count = result.deleted.length;
  const bin = binName(detectPlatform());
  const [first] = result.deleted;
  const inFolder = result.deleted.some((entry) => entry.went_to === "trash_folder");
  const onDisk = result.deleted.some((entry) => entry.went_to !== "nothing_on_disk");
  // Settings › Deleted files is Delete permanently: the files are gone and there is no Undo.
  if (result.deleted.every((entry) => entry.went_to === "erased")) {
    return count === 1 && first !== undefined
      ? t("delete.toast.erased.one", { title: first.title })
      : t("delete.toast.erased.many", { count });
  }
  if (count === 1 && first !== undefined) {
    if (!onDisk) return t("delete.toast.nothingOnDisk", { title: first.title });
    return t(inFolder ? "delete.toast.folder.one" : "delete.toast.one", {
      title: first.title,
      bin,
    });
  }
  return t(inFolder ? "delete.toast.folder.many" : "delete.toast.many", { count, bin });
}

/**
 * Delete (spec `title-deletion`): sends the titles' folders, or only their own files in a shared
 * one, where Settings › Deleted files says and takes them out of the library. It does not ask:
 * the controls go through [`requestDelete`], which confirms first. The
 * titles leave the lists at once (rolled back when the backend refuses them); the toast offers
 * Undo and points at the History entry, so undoing from the toast or from History has the same
 * result. Resolves to whether any title was deleted.
 */
export async function deleteToBin(
  targets: readonly DeleteTarget[],
  options: { onDone?: (() => void) | undefined } = {},
): Promise<boolean> {
  if (targets.length === 0) return false;
  const ids = targets.map((target) => target.id);
  const store = useDeleteStore.getState();
  store.hide(ids);
  try {
    const result = await currentHistoryBackend().deleteTitles(ids);
    // A title the backend left as it is comes back to the lists with the reason.
    store.unhide(result.refused.map((entry) => entry.id));
    if (result.refused.length > 0) {
      notify({
        severity: "warning",
        title: t("delete.refused", { count: result.refused.length }),
        detail: result.refused.map((entry) => `${entry.title}: ${entry.reason}`).join("\n"),
      });
    }
    for (const entry of result.deleted) {
      for (const note of entry.notes) notify({ severity: "info", title: note });
    }
    if (result.deleted.length === 0) {
      store.unhide(ids);
      return false;
    }
    const erased = result.deleted.every((deleted) => deleted.went_to === "erased");
    const entry = erased ? null : result.entry_id;
    showToast({
      glyph: Trash2,
      message: toastFor(result),
      ...(entry === null
        ? {}
        : {
            undo: async () => {
              await undoHistoryEntry(entry);
            },
          }),
    });
    void queryClient.invalidateQueries({ queryKey: HISTORY_KEY });
    await refreshAll();
    store.unhide(result.deleted.map((deleted) => deleted.id));
    options.onDone?.();
    return true;
  } catch (error) {
    store.unhide(ids);
    notify({
      severity: "error",
      title: t("delete.failed", { subject: subjectOf(targets) }),
      detail: messageOf(error),
    });
    return false;
  }
}

/** Opens the one confirmation dialog for a permanent delete (Shift+Delete, "Delete permanently…"). */
export function requestPermanentDelete(
  targets: readonly DeleteTarget[],
  options: { onDone?: (() => void) | undefined } = {},
): void {
  if (targets.length === 0) return;
  useDeleteStore.getState().ask({ targets, onDone: options.onDone });
}

/** Says what a permanent delete did: what went, what stopped at a file, what was refused. */
function reportErased(result: EraseResult): void {
  if (result.erased.length > 0) {
    showToast({ glyph: Trash2, message: t("delete.erased", { count: result.erased.length }) });
  }
  for (const stopped of result.stopped) {
    notify({
      severity: "warning",
      title: t("delete.stopped", { title: stopped.title }),
      detail: stopped.error ?? stopped.left.join("\n"),
    });
  }
  if (result.refused.length > 0) {
    notify({
      severity: "warning",
      title: t("delete.erase.refused", { title: result.refused[0]?.title ?? "" }),
      detail: result.refused.map((entry) => `${entry.title}: ${entry.reason}`).join("\n"),
    });
  }
}

/**
 * Erases the titles' files for good (spec `title-deletion` "Shift+Delete asks for a typed
 * confirmation"). Only the dialog calls this, with what the person typed: the backend erases
 * nothing unless it is exactly `DELETE`. Resolves to whether any title was erased whole.
 */
export async function erasePermanently(
  targets: readonly DeleteTarget[],
  confirm: string,
  options: { onDone?: (() => void) | undefined; password?: string | undefined } = {},
): Promise<boolean> {
  if (targets.length === 0) return false;
  const ids = targets.map((target) => target.id);
  const store = useDeleteStore.getState();
  try {
    const backend = currentHistoryBackend();
    const result =
      options.password === undefined
        ? await backend.eraseTitles(ids, confirm)
        : await backend.eraseTitles(ids, confirm, options.password);
    reportErased(result);
    store.hide(result.erased.map((entry) => entry.id));
    void queryClient.invalidateQueries({ queryKey: HISTORY_KEY });
    await refreshAll();
    store.unhide(result.erased.map((entry) => entry.id));
    if (result.erased.length > 0) options.onDone?.();
    return result.erased.length > 0;
  } catch (error) {
    notify({
      severity: "error",
      title: t("delete.erase.failed"),
      detail: messageOf(error),
    });
    return false;
  }
}
