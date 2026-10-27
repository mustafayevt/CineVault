import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { currentDetailBackend } from "./detailBackend";
import type { DetailCredit } from "./detailBackend";

/** How many times the portraits are asked for again while some are still downloading. */
const RETRIES = 3;
/** Pause before asking again. */
const RETRY_MS = 4000;

const hasPortrait = (credit: DetailCredit) => credit.portrait !== null;
const isCast = (credit: DetailCredit) => credit.kind === "actor" || credit.kind === "guest";

/**
 * The credits of a title with the profile pictures fetched for the people whose portrait is not on
 * disk (OP-1). The pictures are downloaded into the app's cache when the page opens and served
 * over `cv://img`; a portrait file in the media folder (`.actors`) always comes first.
 */
export function useCastPortraits(itemId: string, credits: readonly DetailCredit[]): DetailCredit[] {
  const missing = credits.filter((credit) => isCast(credit) && !hasPortrait(credit)).length;
  const query = useQuery({
    queryKey: ["cast-portraits", itemId],
    queryFn: () => currentDetailBackend().castPortraits(itemId),
    enabled: itemId !== "" && missing > 0,
    staleTime: 60_000,
    // The ones not ready within the first wait keep downloading: ask again a few times.
    refetchInterval: (state) =>
      state.state.dataUpdateCount < RETRIES && (state.state.data?.length ?? 0) < missing
        ? RETRY_MS
        : false,
  });
  const fetched = query.data;
  return useMemo(() => {
    if (fetched === undefined || fetched.length === 0) return [...credits];
    const byName = new Map(fetched.map((entry) => [entry.name.toLowerCase(), entry.portrait]));
    return credits.map((credit) =>
      hasPortrait(credit) || !isCast(credit)
        ? credit
        : { ...credit, portrait: byName.get(credit.name.toLowerCase()) ?? null },
    );
  }, [credits, fetched]);
}

/** A title with its credits, as the detail pages hold it. */
interface WithCredits {
  id: string;
  credits: DetailCredit[];
}

/** The detail of a title with the fetched portraits in its credits. */
export function useDetailWithPortraits<T extends WithCredits>(
  detail: T | undefined,
): T | undefined {
  const merged = useCastPortraits(detail?.id ?? "", detail?.credits ?? []);
  return useMemo(
    () => (detail === undefined ? undefined : { ...detail, credits: merged }),
    [detail, merged],
  );
}
