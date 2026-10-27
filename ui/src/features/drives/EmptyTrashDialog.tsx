import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { HISTORY_KEY } from "../history/historyBackend";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { EraseConfirmDialog } from "../trash/EraseConfirmDialog";
import { DRIVES_KEY, currentDrivesBackend } from "./drivesBackend";

interface EmptyTrashDialogProps {
  /** The drive to empty, or `null` for every drive. */
  driveId: string | null;
  /** "D: Media", for the title; ignored for every drive. */
  name: string;
  onClose: () => void;
}

/**
 * Empty… of a drive, or Empty all trash folders (spec `drives` "Empty a drive's trash"): the
 * typed confirmation with the batches of the trash folders and what CineVault sent to the
 * Recycle Bin, each with its size, and the total. Erasing refreshes the drives and History
 * (entries whose files were erased lose their Undo).
 */
export function EmptyTrashDialog({ driveId, name, onClose }: EmptyTrashDialogProps) {
  const format = useFormat();
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const contents = useQuery({
    queryKey: ["drives", "trash", driveId ?? "*"],
    queryFn: () => currentDrivesBackend().trashContents(driveId),
    staleTime: 0,
    gcTime: 0,
  });
  const entries = contents.data?.entries ?? [];
  const total = contents.data?.bytes ?? 0;

  const confirm = (typed: string) => {
    setBusy(true);
    onClose();
    void currentDrivesBackend()
      .emptyTrash(driveId, typed)
      .then((result) => {
        showToast({ message: t("drivesPage.trash.emptied", { size: format.size(result.bytes) }) });
        if (result.problems.length > 0) {
          notify({
            severity: "warning",
            title: t("drivesPage.trash.problems", { count: result.problems.length }),
            detail: result.problems.join("\n"),
          });
        }
      })
      .catch((error: unknown) =>
        notify({
          severity: "error",
          title: t("drivesPage.trash.failed"),
          detail: error instanceof Error ? error.message : String(error),
        }),
      )
      .finally(() => {
        void client.invalidateQueries({ queryKey: DRIVES_KEY });
        void client.invalidateQueries({ queryKey: HISTORY_KEY });
      });
  };

  return (
    <EraseConfirmDialog
      title={
        driveId === null
          ? t("drivesPage.trash.emptyAllTitle")
          : t("drivesPage.trash.emptyTitle", { name })
      }
      description={
        contents.isPending
          ? t("drivesPage.trash.reading")
          : contents.isError
            ? t("drivesPage.trash.readFailed")
            : entries.length === 0
              ? t("drivesPage.trash.nothing")
              : t("drivesPage.trash.body", { size: format.size(total) })
      }
      confirmLabel={t("drivesPage.trash.confirm")}
      canConfirm={!busy && entries.length > 0}
      onConfirm={confirm}
      onClose={onClose}
    >
      <ul
        aria-label={t("drivesPage.trash.list")}
        className="m-0 flex max-h-[240px] list-none flex-col overflow-y-auto rounded-(--r-10) border border-solid border-line-edge bg-surface-well p-0"
      >
        {contents.isPending ? (
          <li aria-busy="true" className="p-3">
            <Skeleton shape="line" width="100%" height="34px" />
          </li>
        ) : (
          entries.map((entry) => (
            <li
              key={`${entry.kind}:${entry.path}`}
              className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 border-t border-solid border-line-row px-3.5 py-2.5 first:border-t-0"
            >
              <div className="min-w-0">
                <div className="text-(length:--fs-11) font-(weight:--fw-600) tracking-[0.06em] text-text-muted uppercase">
                  {entry.kind === "bin"
                    ? t("drivesPage.trash.inBin")
                    : t("drivesPage.trash.inFolder")}
                </div>
                <div
                  className="cv-selectable truncate font-mono text-(length:--fs-12) text-text-2"
                  title={entry.path}
                >
                  {entry.path}
                </div>
              </div>
              <span className="text-right text-text-2 tabular-nums">
                {format.size(entry.bytes)}
              </span>
            </li>
          ))
        )}
      </ul>
    </EraseConfirmDialog>
  );
}
