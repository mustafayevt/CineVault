import { useMemo } from "react";

import { useNotificationStore } from "../notifications/notificationStore";
import type { AppNotification } from "../notifications/notificationStore";
import { useUnfinishedBatches } from "./recoveryBackend";
import type { UnfinishedBatch } from "./recoveryBackend";
import { useRecoveryStore } from "./resolveUnfinished";

/** One row of Activity's Needs attention. */
export type AttentionEntry =
  | { kind: "batch"; key: string; batch: UnfinishedBatch }
  | { kind: "notice"; key: string; notice: AppNotification };

/**
 * What needs the user, in the order it is shown: unfinished moves and renames first (they hold
 * titles back from other work), the one being resumed among them, then warnings and errors,
 * newest first. Information is not listed; it was said in a toast.
 *
 * Once the backend's list is read, it is the truth about unfinished batches: a notice about one
 * (the start-up warning, "The move of Heat stopped") adds nothing to its row, and a notice about
 * one that is resolved now is out of date, so neither is listed. While the list cannot be read,
 * those notices stand in for it.
 */
export function attentionEntries(
  batches: readonly UnfinishedBatch[] | undefined,
  active: UnfinishedBatch | null,
  notices: readonly AppNotification[],
): AttentionEntry[] {
  const listed = batches ?? [];
  const rows =
    active !== null && !listed.some((batch) => batch.batch_id === active.batch_id)
      ? [active, ...listed]
      : listed;
  const entries: AttentionEntry[] = rows.map((batch) => ({
    kind: "batch",
    key: `batch:${batch.batch_id}`,
    batch,
  }));
  for (const notice of notices) {
    if (notice.severity === "info") continue;
    if (batches !== undefined && notice.action === "resolve_unfinished") continue;
    entries.push({ kind: "notice", key: `notice:${notice.id}`, notice });
  }
  return entries;
}

/** The rows of Needs attention, live: the sidebar's Activity badge counts them. */
export function useNeedsAttention(): AttentionEntry[] {
  const batches = useUnfinishedBatches().data;
  const active = useRecoveryStore((state) => state.active);
  const notices = useNotificationStore((state) => state.items);
  return useMemo(() => attentionEntries(batches, active, notices), [batches, active, notices]);
}
