import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderPen } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import type {
  ChoiceDto,
  OrganizePreviewDto,
  OrganizeRequestDto,
  PatternsDto,
  UndoOutcomeDto,
} from "../../bindings";
import { t } from "../../i18n/catalog";
import { undoBatchInHistory } from "../history/historyUndo";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentOrganizeBackend } from "./organizeBackend";

/** Query key of everything the Rename page reads; anything that renames or reads details invalidates it. */
export const RENAME_KEY = ["rename"] as const;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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
 * The plan of the Rename page (spec `renamer` "Rename page preview"): the active naming profile's
 * templates, the plan for them over the library or the titles of `?ids=`, the resolutions the user
 * chose for conflicts, and applying the titles that are ticked. Nothing touches the disk until
 * `apply`; the plan is read again whenever a resolution changes or something renames.
 */
export function useRenamePlan(scope: readonly string[] | null) {
  const backend = currentOrganizeBackend();
  const queryClient = useQueryClient();
  const [choices, setChoices] = useState<Readonly<Record<string, ChoiceDto>>>({});
  const [applying, setApplying] = useState(false);

  const profiles = useQuery({
    queryKey: [...RENAME_KEY, "profiles"],
    queryFn: () => backend.profiles(),
    staleTime: 0,
  });
  const active =
    profiles.data?.profiles.find((profile) => profile.id === profiles.data?.active) ??
    profiles.data?.profiles[0];
  const patterns: PatternsDto | null = active?.patterns ?? null;

  const request = useMemo<OrganizeRequestDto | null>(
    () =>
      active === undefined
        ? null
        : {
            scope: scope === null ? { kind: "everything" } : { kind: "selection", ids: [...scope] },
            profile_id: active.id,
            patterns: active.patterns,
            choices: Object.values(choices),
          },
    [active, scope, choices],
  );

  const plan = useQuery({
    queryKey: [...RENAME_KEY, "plan", request],
    queryFn: () => {
      if (request === null) throw new Error("no naming profile");
      return backend.preview(request);
    },
    enabled: request !== null,
    staleTime: 0,
    placeholderData: keepPreviousData,
  });

  const choose = useCallback((choice: ChoiceDto) => {
    setChoices((current) => ({ ...current, [choice.id]: choice }));
  }, []);
  const clearChoice = useCallback((id: string) => {
    setChoices((current) =>
      Object.fromEntries(Object.entries(current).filter(([key]) => key !== id)),
    );
  }, []);

  /** Everything that shows names or stages, read again after a rename or an undo. */
  const refresh = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: RENAME_KEY }),
      queryClient.invalidateQueries({ queryKey: ["lifecycle"] }),
      queryClient.invalidateQueries({ queryKey: ["library"] }),
      queryClient.invalidateQueries({ queryKey: ["movie-detail"] }),
      queryClient.invalidateQueries({ queryKey: ["tv-show"] }),
    ]);
  }, [queryClient]);

  /** Renames the titles with these ids and says so with an Undo toast. */
  const apply = useCallback(
    async (ids: readonly string[]) => {
      if (request === null || applying || ids.length === 0) return;
      setApplying(true);
      try {
        const applied = await backend.apply(request, ids);
        const batchId = applied.batch_id;
        if (batchId === null) {
          showToast({ message: t("renamePage.toast.nothing") });
        } else {
          const done = t("renamePage.toast.done", { count: applied.done });
          showToast({
            glyph: FolderPen,
            message:
              applied.failed === 0
                ? done
                : t("renamePage.toast.partly", { done, count: applied.failed }),
            undo: async () => {
              // The rename's History entry is the record, so the toast and the History page undo
              // the same thing; a batch no entry records is undone through its journal.
              const report = await undoBatchInHistory(batchId);
              const outcome =
                report === null
                  ? await backend.undo(batchId)
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
              await refresh();
            },
          });
        }
        const failures = applied.items.filter((item) => item.status === "failed");
        if (failures.length > 0) {
          notify({
            severity: "error",
            title: t("renamePage.toast.couldNot"),
            detail: failures.map((item) => `${item.title}: ${item.error}`).join("\n"),
          });
        }
      } catch (error) {
        notify({
          severity: "error",
          title: t("renamePage.toast.couldNot"),
          detail: messageOf(error),
        });
      } finally {
        setApplying(false);
        await refresh();
      }
    },
    [applying, backend, refresh, request],
  );

  const preview: OrganizePreviewDto | null = plan.data ?? null;
  return {
    patterns,
    preview,
    loading: profiles.isPending || (plan.isPending && request !== null),
    error: profiles.error ?? plan.error,
    refetch: () => {
      void profiles.refetch();
      void plan.refetch();
    },
    choose,
    clearChoice,
    apply,
    applying,
  };
}
