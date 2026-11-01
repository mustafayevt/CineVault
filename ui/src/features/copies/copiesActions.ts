import { Copy } from "lucide-react";

import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { requestDelete } from "../delete/deleteActions";
import type { DeleteTarget } from "../delete/deleteActions";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentCopiesBackend } from "./copiesBackend";
import type { Comparison, CopyRow } from "./copiesBackend";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** "Example Movie (2020)", or the title alone when the year is unknown. */
export function copyName(copy: Pick<CopyRow, "title" | "year">): string {
  return copy.year > 0 ? `${copy.title} (${copy.year})` : copy.title;
}

/** What the page does once a copy is kept: leave the page, or open the kept copy's page. */
export interface KeepOutcome {
  /** Runs after the other copies were deleted. */
  done: () => void;
  /** Opens the Rename preview of the kept movie alone. */
  rename: () => void;
}

/**
 * "Keep this one" (spec `movie-copies`): deletes every other copy with the existing Delete, so it
 * follows Settings › Deleted files with the same confirmation, toast, History entry and Undo. The
 * kept copy is not touched. When the kept copy carries a copy label in its names, the toast also
 * offers to drop it; that opens the Rename preview and renames nothing by itself.
 */
export async function keepThisOne(
  page: Comparison,
  keepId: string,
  outcome: KeepOutcome,
): Promise<void> {
  const targets: DeleteTarget[] = page.copies
    .filter((copy) => copy.id !== keepId)
    .map((copy) => ({ id: copy.id, title: copyName(copy) }));
  // Asked before anything moves: the label is a property of the kept copy's names, not of the
  // delete. A failed read just leaves the offer out.
  const label = await currentCopiesBackend()
    .labelToDrop(keepId)
    .catch(() => null);
  requestDelete(targets, {
    onDone: outcome.done,
    ...(label === null || label === ""
      ? {}
      : { toastAction: { label: t("copies.dropLabel", { label }), onClick: outcome.rename } }),
  });
}

/** Refreshes everything that shows copies: the chip, the badges, the lines and the page. */
async function refreshCopies(): Promise<void> {
  await queryClient.invalidateQueries();
}

/**
 * "Keep both" / "Keep all": records that the user keeps these copies. No file changes. The toast
 * offers Undo, which asks again.
 */
export async function keepBoth(page: Comparison): Promise<boolean> {
  const backend = currentCopiesBackend();
  const first = page.copies[0];
  if (first === undefined) return false;
  try {
    await backend.keepAll(page.copies.map((copy) => copy.id));
    await refreshCopies();
  } catch (error) {
    notify({ severity: "error", title: t("copies.failed"), detail: messageOf(error) });
    return false;
  }
  const name = copyName(first);
  showToast({
    glyph: Copy,
    message:
      page.copies.length === 2
        ? t("copies.toast.kept.two", { title: name })
        : t("copies.toast.kept.many", { title: name, count: page.copies.length }),
    undo: async () => {
      await backend.askAgain(first.id);
      await refreshCopies();
    },
  });
  return true;
}

/** "Ask again": the group counts as copies again. */
export async function askAgain(page: Comparison): Promise<boolean> {
  const first = page.copies[0];
  if (first === undefined) return false;
  try {
    await currentCopiesBackend().askAgain(first.id);
    await refreshCopies();
  } catch (error) {
    notify({ severity: "error", title: t("copies.failed"), detail: messageOf(error) });
    return false;
  }
  showToast({ glyph: Copy, message: t("copies.toast.askedAgain", { title: copyName(first) }) });
  return true;
}
