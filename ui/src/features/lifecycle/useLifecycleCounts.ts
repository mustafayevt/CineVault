import { useQuery } from "@tanstack/react-query";

import { currentLifecycleBackend } from "./lifecycleBackend";
import type { PipelineKind } from "./lifecycleBackend";

/** Query key of the pipeline counts of a library type; invalidate it when a step finishes. */
function lifecycleCountsKey(kind: PipelineKind) {
  return ["lifecycle", "counts", kind] as const;
}

/**
 * The pipeline counts of a library type: titles per stage, and Needs review, Not matched,
 * Working, Rename pending, Locked and Missing. Every count that opens the library filtered to
 * those titles reads this hook, so two places never show different numbers for one set.
 */
export function useLifecycleCounts(kind: PipelineKind) {
  return useQuery({
    queryKey: lifecycleCountsKey(kind),
    queryFn: () => currentLifecycleBackend().counts(kind),
    staleTime: 30_000,
  });
}
