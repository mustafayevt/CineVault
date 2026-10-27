import { useEffect, useState } from "react";
import { create } from "zustand";

import { queryClient } from "../../queryClient";
import { REVIEW_COUNT_KEY, REVIEW_QUEUE_KEY } from "../scrape/useScrapeQueue";
import { currentProvidersBackend } from "./providersBackend";
import type { KeyAttention } from "./providersBackend";

/** Counts the saves that changed what the banner and the provider list say. */
const useProvidersRevision = create<{ revision: number }>(() => ({ revision: 0 }));

/**
 * Call after a key was saved or removed. A banner that is on screen reads the state again, and the
 * queries that the matching the key just started will change (the Needs review queue and count,
 * the library) are read again too: saving the TMDB key resumes matching, so titles arrive soon.
 */
export function providersChanged(): void {
  useProvidersRevision.setState((state) => ({ revision: state.revision + 1 }));
  for (const key of [REVIEW_QUEUE_KEY, REVIEW_COUNT_KEY, ["library"]]) {
    void queryClient.invalidateQueries({ queryKey: key });
  }
}

/** The banner's content, read from the backend when the page opens and after a key changed. */
export function useKeyAttention(): KeyAttention | null {
  const revision = useProvidersRevision((state) => state.revision);
  const [attention, setAttention] = useState<KeyAttention | null>(null);
  useEffect(() => {
    let live = true;
    currentProvidersBackend()
      .attention()
      .then((next) => {
        if (live) setAttention(next);
      })
      .catch(() => {
        // Not in the desktop app, or the list could not be read: no banner.
      });
    return () => {
      live = false;
    };
  }, [revision]);
  return attention;
}
