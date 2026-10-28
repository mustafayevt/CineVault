import { useQuery } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Field } from "../../components/Field";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useSetting } from "../settings/settingsStore";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import type { RemovalPlan } from "../library-ops/libraryOpsBackend";
import { binName } from "../shell/binName";
import { detectPlatform } from "../shell/platform";
import { deleteToBin, erasePermanently, useDeleteStore } from "./deleteActions";
import type { DeleteTarget } from "./deleteActions";

/** The word that has to be typed. The backend checks it again. */
const CONFIRM_WORD = "DELETE";

/** "Heat (1995)", or the title alone without a year. */
function nameOf(row: { title: string; year: number }): string {
  return row.year > 0 ? `${row.title} (${row.year})` : row.title;
}

/**
 * The permanent-delete dialog (spec `title-deletion` "Shift+Delete asks for a typed confirmation"), also
 * what Delete opens when Settings › Deleted files is Delete permanently: how many titles, each with its folder and size on a well list, the words
 * "This skips the bin and cannot be undone" (the bin is the Recycle Bin on Windows, the Trash
 * elsewhere), a field for DELETE, and the buttons Cancel, "Send to the bin instead" and "Delete
 * permanently", which stays off until the field holds
 * exactly DELETE. The list is what the backend says it will touch; the titles it would refuse
 * (an offline drive) are named apart and are not sent.
 */
export function PermanentDeleteDialog() {
  const request = useDeleteStore((state) => state.request);
  const close = useDeleteStore((state) => state.close);
  return request === null ? null : (
    <Confirmation
      key={request.targets.map((target) => target.id).join(",")}
      targets={request.targets}
      onDone={request.onDone}
      onClose={close}
    />
  );
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
  const bin = binName(detectPlatform());
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  // Under Delete permanently a delete erases too, so there is no bin to offer instead.
  const erasing = useSetting<string>("files.deleted-mode") === "erase";
  const ids = targets.map((target) => target.id);

  const plan = useQuery<RemovalPlan>({
    queryKey: ["delete", "plan", ids.join(",")],
    queryFn: () => currentLibraryOpsBackend().planRemoval(ids),
    staleTime: 0,
    gcTime: 0,
  });
  const rows = plan.data?.rows ?? [];
  const skipped = plan.data?.skipped ?? [];
  const files = rows.reduce((sum, row) => sum + row.files, 0);

  const ready = typed === CONFIRM_WORD && rows.length > 0 && !busy;

  const confirm = async () => {
    if (!ready) return;
    setBusy(true);
    const listed = rows.map((row) => ({ id: row.id, title: nameOf(row) }));
    onClose();
    await erasePermanently(listed, typed, { onDone });
  };
  const sendToBin = () => {
    onClose();
    void deleteToBin(targets, { onDone });
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      hideClose
      className="w-[520px]!"
      title={
        <span className="flex items-center gap-2">
          <Trash2 aria-hidden="true" size={18} className="shrink-0 text-danger" />
          {t("delete.dialog.title", { count: targets.length })}
        </span>
      }
      description={
        plan.isPending
          ? t("delete.dialog.reading")
          : plan.isError
            ? t("delete.dialog.planFailed")
            : t("delete.dialog.body", {
                count: rows.length,
                files: t("delete.dialog.files", { count: files }),
                bin,
              })
      }
      footer={
        <div className="flex w-full items-center gap-2">
          {erasing ? null : (
            <Button variant="ghost" onClick={sendToBin} disabled={busy}>
              {t("delete.dialog.toBin", { bin })}
            </Button>
          )}
          <span className="flex-1" />
          <Button size="lg" onClick={onClose}>
            {t("delete.dialog.cancel")}
          </Button>
          <Button
            size="lg"
            variant="danger"
            disabled={!ready}
            onClick={() => void confirm()}
            data-testid="confirm-erase"
          >
            {t("delete.dialog.confirm")}
          </Button>
        </div>
      }
    >
      <ul
        aria-label={t("delete.dialog.list")}
        className="m-0 flex max-h-[240px] list-none flex-col overflow-y-auto rounded-(--r-10) border border-solid border-line-edge bg-surface-well p-0"
      >
        {plan.isPending ? (
          <li aria-busy="true" className="p-3">
            <Skeleton shape="line" width="100%" height="34px" />
          </li>
        ) : (
          <>
            {rows.map((row) => (
              <li
                key={row.id}
                className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 border-t border-solid border-line-row px-3.5 py-2.5 first:border-t-0"
              >
                <div className="min-w-0">
                  <div className="truncate font-(weight:--fw-600) text-text">{nameOf(row)}</div>
                  <div className="cv-selectable truncate font-mono text-(length:--fs-12) text-text-muted">
                    {row.folder === "" ? t("delete.dialog.noFolder") : row.folder}
                  </div>
                  {row.folder_use === "shared" || row.folder_use === "source_root" ? (
                    <div className="text-(length:--fs-12) text-warn">
                      {t(
                        row.folder_use === "shared" ? "delete.dialog.shared" : "delete.dialog.root",
                      )}
                    </div>
                  ) : null}
                </div>
                <span className="text-right text-text-2 tabular-nums">
                  {row.bytes === null ? t("common.unknownSize") : format.size(row.bytes)}
                </span>
              </li>
            ))}
            {skipped.map((problem) => (
              <li
                key={problem.id}
                className="border-t border-solid border-line-row px-3.5 py-2.5 first:border-t-0"
              >
                <div className="text-(length:--fs-11) font-(weight:--fw-600) tracking-[0.06em] text-text-muted uppercase">
                  {t("delete.dialog.skipped")}
                </div>
                <div className="truncate font-(weight:--fw-600) text-text-2">{problem.title}</div>
                <div className="text-(length:--fs-12) text-text-muted">{problem.reason}</div>
              </li>
            ))}
          </>
        )}
      </ul>
      <div className="flex flex-col gap-1.5">
        <label htmlFor="delete-confirm" className="text-text-2">
          {t("delete.dialog.typeBefore")}{" "}
          <b className="font-mono font-(weight:--fw-700) text-text">{CONFIRM_WORD}</b>{" "}
          {t("delete.dialog.typeAfter")}
        </label>
        <Field
          id="delete-confirm"
          mono
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && ready) void confirm();
          }}
          autoComplete="off"
          spellCheck={false}
          aria-label={t("delete.dialog.field")}
        />
      </div>
    </Dialog>
  );
}
