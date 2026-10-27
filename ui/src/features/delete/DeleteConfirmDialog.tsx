import { useQuery } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import type { KeyboardEvent } from "react";

import { Button } from "../../components/Button";
import { Checkbox } from "../../components/Checkbox";
import { Dialog } from "../../components/Dialog";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import type { RemovalPlan } from "../library-ops/libraryOpsBackend";
import type { DeletedMode } from "../settings/DeletedFilesSection";
import { useSetting, useSettingsStore } from "../settings/settingsStore";
import { binName } from "../shell/binName";
import { detectPlatform } from "../shell/platform";
import { deleteToBin, useDeleteStore } from "./deleteActions";
import type { DeleteTarget } from "./deleteActions";

/** How many titles the list names; the rest are counted. */
const LISTED = 5;

/** "Heat (1995)", or the title alone without a year. */
function nameOf(row: { title: string; year: number }): string {
  return row.year > 0 ? `${row.title} (${row.year})` : row.title;
}

/**
 * The Delete confirmation (spec `title-deletion`): the title or the number of titles, where the
 * files go under Settings › Deleted files, up to five titles with their sizes and the total, a
 * "Don't ask again" box, and Cancel · Delete. Focus starts on Delete, so Enter deletes at once;
 * Esc and Cancel leave everything as it is. Delete permanently never comes here: it always asks
 * for the typed DELETE in the permanent-delete dialog.
 */
export function DeleteConfirmDialog() {
  const request = useDeleteStore((state) => state.confirm);
  const close = useDeleteStore((state) => state.closeConfirm);
  return request === null ? null : (
    <Confirmation
      key={request.targets.map((target) => target.id).join(",")}
      targets={request.targets}
      onDone={request.onDone}
      onClose={close}
    />
  );
}

function bodyOf(mode: DeletedMode, count: number, folder: string): string {
  if (mode === "folder") return t("delete.confirm.body.folder", { count });
  if (mode === "custom") {
    return folder === ""
      ? t("delete.confirm.body.customUnset", { count })
      : t("delete.confirm.body.custom", { count, folder });
  }
  return t("delete.confirm.body.bin", { count, bin: binName(detectPlatform()) });
}

function Confirmation({
  targets,
  onDone,
  onClose,
}: {
  targets: readonly DeleteTarget[];
  onDone: (() => void) | undefined;
  onClose: () => void;
}) {
  const format = useFormat();
  const mode = useSetting<string>("files.deleted-mode") as DeletedMode;
  const folder = useSetting<string>("files.trash-folder");
  const [dontAsk, setDontAsk] = useState(false);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const ids = targets.map((target) => target.id);
  const [only] = targets;

  const plan = useQuery<RemovalPlan>({
    queryKey: ["delete", "confirm-plan", ids.join(",")],
    queryFn: () => currentLibraryOpsBackend().planRemoval(ids),
    staleTime: 0,
    gcTime: 0,
  });
  const rows = plan.data?.rows ?? [];
  const listed = rows.slice(0, LISTED);
  const more = rows.length - listed.length;
  const total = rows.some((row) => row.bytes === null)
    ? null
    : rows.reduce((sum, row) => sum + (row.bytes ?? 0), 0);

  const confirm = () => {
    if (dontAsk) useSettingsStore.getState().set("delete.confirm", false);
    onClose();
    void deleteToBin(targets, { onDone });
  };
  // Enter on the "Don't ask again" box deletes too, as it does on the Delete button.
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    confirm();
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      hideClose
      // Focus starts on Delete so Enter deletes; Esc and Cancel are always there.
      initialFocus={deleteButton}
      className="w-[480px]!"
      title={
        <span className="flex items-center gap-2">
          <Trash2
            aria-hidden="true"
            size={18}
            strokeWidth={1.75}
            className="shrink-0 text-danger"
          />
          {targets.length === 1 && only !== undefined
            ? t("delete.confirm.title.one", { title: only.title })
            : t("delete.confirm.title.many", { count: targets.length })}
        </span>
      }
      description={bodyOf(mode, targets.length, folder)}
      footer={
        <>
          <Button size="lg" onClick={onClose}>
            {t("delete.confirm.cancel")}
          </Button>
          <Button
            size="lg"
            variant="danger"
            onClick={confirm}
            ref={deleteButton}
            data-testid="confirm-delete"
          >
            {t("delete.confirm.confirm")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-2">
        <ul
          aria-label={t("delete.confirm.list")}
          className="m-0 flex list-none flex-col rounded-(--r-10) border border-solid border-line-edge bg-surface-well p-0"
        >
          {plan.isPending ? (
            <li aria-busy="true" className="p-3">
              <Skeleton shape="line" width="100%" height="20px" />
            </li>
          ) : plan.isError ? (
            targets.slice(0, LISTED).map((target) => (
              <li
                key={target.id}
                className="truncate border-t border-solid border-line-row px-3.5 py-2 text-text first:border-t-0"
              >
                {target.title}
              </li>
            ))
          ) : (
            <>
              {listed.map((row) => (
                <li
                  key={row.id}
                  className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 border-t border-solid border-line-row px-3.5 py-2 first:border-t-0"
                >
                  <span className="truncate text-text">{nameOf(row)}</span>
                  <span className="text-right text-text-2 tabular-nums">
                    {row.bytes === null ? t("common.unknownSize") : format.size(row.bytes)}
                  </span>
                </li>
              ))}
              {more > 0 ? (
                <li className="border-t border-solid border-line-row px-3.5 py-2 text-text-muted">
                  {t("delete.confirm.more", { count: more })}
                </li>
              ) : null}
            </>
          )}
        </ul>
        {!plan.isPending && !plan.isError && rows.length > 1 ? (
          <div className="flex justify-between px-3.5 text-text-2">
            <span>{t("delete.confirm.total")}</span>
            <span className="tabular-nums" data-testid="delete-total">
              {total === null ? t("common.unknownSize") : format.size(total)}
            </span>
          </div>
        ) : null}
        <Checkbox
          checked={dontAsk}
          onCheckedChange={setDontAsk}
          onKeyDown={onKeyDown}
          className="pt-1"
        >
          {t("delete.confirm.dontAsk")}
        </Checkbox>
      </div>
    </Dialog>
  );
}
