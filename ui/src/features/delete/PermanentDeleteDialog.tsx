import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { isServer } from "../../lib/transport";
import { useFormat } from "../../i18n/useFormat";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import type { RemovalPlan } from "../library-ops/libraryOpsBackend";
import { EraseConfirmDialog } from "../trash/EraseConfirmDialog";
import { erasePermanently, useDeleteStore } from "./deleteActions";
import type { DeleteTarget } from "./deleteActions";

/** "Heat (1995)", or the title alone without a year. */
function nameOf(row: { title: string; year: number }): string {
  return row.year > 0 ? `${row.title} (${row.year})` : row.title;
}

/**
 * The permanent-delete dialog (spec `title-deletion` "A permanent delete asks Yes or No"), what
 * Delete opens when Settings › Deleted files is Delete permanently: how many titles, each with its
 * folder and size on a well list and the total, the words "This cannot be undone", and the buttons
 * Cancel and "Delete permanently". In a browser it also asks for the current password. The list is
 * what the backend says it will touch; the titles it would refuse (an offline drive) are named
 * apart and are not sent.
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
  const [password, setPassword] = useState("");
  const browser = isServer();
  const [busy, setBusy] = useState(false);
  const ids = targets.map((target) => target.id);

  const plan = useQuery<RemovalPlan>({
    queryKey: ["delete", "plan", ids.join(",")],
    queryFn: () => currentLibraryOpsBackend().planRemoval(ids),
    staleTime: 0,
    gcTime: 0,
  });
  const rows = plan.data?.rows ?? [];
  const skipped = plan.data?.skipped ?? [];
  const total = rows.some((row) => row.bytes === null)
    ? null
    : rows.reduce((sum, row) => sum + (row.bytes ?? 0), 0);

  const ready = rows.length > 0 && !busy && (!browser || password.length > 0);

  const confirm = async () => {
    setBusy(true);
    const listed = rows.map((row) => ({ id: row.id, title: nameOf(row) }));
    onClose();
    await erasePermanently(listed, { onDone, password: browser ? password : undefined });
  };

  return (
    <EraseConfirmDialog
      title={t("delete.dialog.title", { count: targets.length })}
      description={
        plan.isPending
          ? t("delete.dialog.reading")
          : plan.isError
            ? t("delete.dialog.planFailed")
            : t("delete.dialog.body", { count: rows.length })
      }
      confirmLabel={t("delete.dialog.confirm")}
      canConfirm={ready}
      onConfirm={() => void confirm()}
      onClose={onClose}
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
      {!plan.isPending && !plan.isError && rows.length > 1 ? (
        <div className="flex justify-between px-3.5 text-text-2">
          <span>{t("delete.confirm.total")}</span>
          <span className="tabular-nums">
            {total === null ? t("common.unknownSize") : format.size(total)}
          </span>
        </div>
      ) : null}
      {browser ? (
        <div className="flex flex-col gap-1.5">
          <label htmlFor="delete-password" className="text-text-2">
            {t("delete.dialog.password")}
          </label>
          <input
            id="delete-password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            className="h-8 rounded-(--r-8) border border-solid border-line-edge bg-surface-well px-2.5 text-(length:--fs-13)"
          />
        </div>
      ) : null}
    </EraseConfirmDialog>
  );
}
