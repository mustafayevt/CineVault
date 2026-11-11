import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";

import type {
  ChoiceDto,
  OrganizePreviewDto,
  OrganizeRequestDto,
  PatternsDto,
} from "../../bindings";
import { currentOrganizeBackend } from "./organizeBackend";
import { RENAME_KEY, runRename, useRenameRunStore } from "./runRename";

export { RENAME_KEY } from "./runRename";

/**
 * The plan of the Rename page (spec `renamer` "Rename page preview"): the active naming profile's
 * templates, the plan for them over the library or the titles of `?ids=`, the resolutions the user
 * chose for conflicts, and applying the titles that are ticked. Nothing touches the disk until
 * `apply`; the plan is read again whenever a resolution changes or something renames. The rename
 * itself runs in [`runRename`], so it reports when it ends even after the page was left.
 */
export function useRenamePlan(scope: readonly string[] | null) {
  const backend = currentOrganizeBackend();
  const client = useQueryClient();
  const [choices, setChoices] = useState<Readonly<Record<string, ChoiceDto>>>({});
  const applying = useRenameRunStore((state) => state.running !== null);

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

  /** Renames the titles with these ids; the result is told by a toast wherever the user is. */
  const apply = useCallback(
    async (ids: readonly string[]) => {
      if (request === null || ids.length === 0) return;
      await runRename(request, ids, client);
    },
    [client, request],
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
