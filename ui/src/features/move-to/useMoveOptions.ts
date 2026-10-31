import { useQueries, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { useFormat } from "../../i18n/useFormat";
import { useDrivesOverview } from "../drives/drivesBackend";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import type { MovePlan } from "../library-ops/libraryOpsBackend";
import { buildOption, driveOfDestination, sourceFrees } from "./moveOptions";
import type { MoveOption, SourceFrees } from "./moveOptions";
import type { MoveKind } from "./moveToStore";

export interface MoveOptions {
  /** True until the folders and their plans have been read. */
  loading: boolean;
  /** True when the library folders could not be read. */
  failed: boolean;
  /** Reads the folders and the plans again (after a failure). */
  retry: () => void;
  options: readonly MoveOption[];
  /** How many bytes the request carries in all, when the plans say. */
  totalBytes: number | null;
  /** What the source drive frees if the first destination that can take the titles is chosen. */
  frees: SourceFrees | null;
}

/**
 * The destinations of a move and the numbers the Move to menu shows (spec `source-moves` "Move to
 * menu with space before and after"): every other library folder of the same type, each with the
 * backend's plan for these titles, joined with the drives' names and free space. Reads the plans;
 * nothing touches the disk.
 */
export function useMoveOptions(
  ids: readonly string[],
  kind: MoveKind,
  enabled = true,
): MoveOptions {
  const format = useFormat();
  const backend = currentLibraryOpsBackend();
  const drives = useDrivesOverview().data?.drives;
  const type = kind === "movie" ? "movie" : "tv";
  const idsKey = ids.join(",");

  const sources = useQuery({
    queryKey: ["move-to", "sources"],
    queryFn: () => backend.listSources(),
    enabled,
    staleTime: 0,
  });
  const destinations = useMemo(
    () => (sources.data ?? []).filter((source) => source.kind === type),
    [sources.data, type],
  );
  const plans = useQueries({
    queries: destinations.map((source) => ({
      queryKey: ["move-to", "plan", idsKey, source.id] as const,
      queryFn: (): Promise<MovePlan> => backend.planMove(ids, source.id),
      enabled,
      staleTime: 0,
      gcTime: 0,
    })),
  });

  const settled = plans.every((plan) => plan.data !== undefined || plan.isError);
  const loading = (sources.isPending && !sources.isError) || !settled;
  const planKey = plans.map((plan) => plan.dataUpdatedAt).join();

  return useMemo<MoveOptions>(() => {
    const built: { option: MoveOption; plan: MovePlan }[] = [];
    destinations.forEach((source, index) => {
      const plan = plans[index]?.data;
      if (plan === undefined) return;
      // The folder the titles are in already is not a destination.
      if (plan.rows.length > 0 && plan.rows.every((row) => row.status === "same_source")) return;
      const drive = drives === undefined ? undefined : driveOfDestination(drives, source);
      built.push({ option: buildOption(source, plan, drive, format.size), plan });
    });
    // The size of the request: the folder the titles are in already reports nothing to move, so
    // the largest plan that knows every title's size is the one that says.
    let totalBytes: number | null = null;
    for (const plan of plans) {
      const rows = plan.data?.rows;
      if (rows === undefined || rows.length === 0 || rows.some((row) => row.bytes === null)) {
        continue;
      }
      const sum = rows.reduce((total, row) => total + (row.bytes ?? 0), 0);
      if (totalBytes === null || sum > totalBytes) totalBytes = sum;
    }
    const best = built.find(({ option }) => option.reason === null);
    return {
      loading,
      failed: sources.isError,
      retry: () => void sources.refetch(),
      options: built.map(({ option }) => option),
      totalBytes,
      frees:
        best === undefined || drives === undefined
          ? null
          : sourceFrees(best.plan, drives, best.option.movedBytes, type),
    };
    // `planKey` stands for the plans' arrival.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [destinations, planKey, drives, type, format, loading, sources.isError]);
}
