import { useMemo } from "react";

import { useSetting } from "../settings/settingsStore";
import { useDebounced } from "../workspaces/useDebounced";
import { useProviderStream } from "../workspaces/useProviderStream";
import { currentScrapeBackend } from "./scrapeBackend";
import type { ScrapeFieldDiff, ScrapeStreamHit } from "./scrapeBackend";
import { mergeDiffRows } from "./scrapeModel";

/** How long the choice rests before its details are asked for (arrowing through the list is free). */
const REST_MS = 450;

/**
 * Every field the provider has for `hit`, as the preview of scraping with it: `null` until the
 * details have arrived (and while the choice is still moving), empty when the provider had none.
 * The preview writes nothing.
 */
export function useCandidatePreview(
  itemId: string,
  hit: ScrapeStreamHit | null,
  enabled = true,
): ScrapeFieldDiff[] | null {
  const backend = currentScrapeBackend();
  const language = useSetting<string>("metadata.language");
  const transport = useMemo(() => backend.previewTransport(), [backend]);
  const settled = useDebounced(hit?.id ?? "", REST_MS);
  const ready = enabled && hit !== null && settled === hit.id;
  const candidate = useMemo(
    () =>
      hit === null
        ? ""
        : JSON.stringify({
            provider: hit.candidate.provider,
            providerId: hit.candidate.providerId,
            title: hit.candidate.title,
            year: hit.candidate.year,
            runtimeMinutes: hit.candidate.runtimeMinutes,
            overview: hit.candidate.overview,
          }),
    [hit],
  );
  const preview = useProviderStream({
    transport,
    query: ready ? settled : "",
    params: { itemId, language, candidate },
    enabled: ready,
  });
  const rows = useMemo(() => mergeDiffRows(preview.items), [preview.items]);
  if (!ready || preview.searching) return null;
  return rows;
}

/** How many artwork files scraping would write, from the preview `rows`; `null` until they arrive. */
export function artworkCountOf(rows: readonly ScrapeFieldDiff[] | null): number | null {
  if (rows === null || rows.length === 0) return null;
  return rows.filter((row) => row.group === "artwork" && row.incoming !== null).length;
}

/** How many artwork files scraping with `hit` would write: `null` until the details arrive. */
export function useArtworkCount(
  itemId: string,
  hit: ScrapeStreamHit | null,
  enabled = true,
): number | null {
  return artworkCountOf(useCandidatePreview(itemId, hit, enabled));
}
