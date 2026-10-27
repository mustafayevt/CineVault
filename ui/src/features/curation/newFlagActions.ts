import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { currentCurationBackend } from "./curationBackend";

/** Queries that show the NEW marker: they are read again after a clear and after an undo. */
const NEW_QUERIES = ["library", "tv-show", "tv-episode", "movie-detail"] as const;

function refresh(client: { invalidateQueries: (filter: { queryKey: string[] }) => Promise<void> }) {
  return Promise.all(NEW_QUERIES.map((key) => client.invalidateQueries({ queryKey: [key] })));
}

/**
 * A title is NEW until it is opened: opening a detail page clears its marker, quietly. Nothing is
 * shown and nothing is refreshed unless the title was marked.
 */
export function useClearNewOnOpen(id: string): void {
  const client = useQueryClient();
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const changed = await currentCurationBackend().clearNewOnOpen(id);
        if (!cancelled && changed.length > 0) await refresh(client);
      } catch {
        // The marker staying is harmless; the page must not complain about it.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [id, client]);
}
