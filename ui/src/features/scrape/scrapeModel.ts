import type { ProviderItem } from "../workspaces/providerEvents";
import type { LibraryLink, ScrapeFieldDiff, ScrapeStreamHit, ScrapeTarget } from "./scrapeBackend";

/** `Solaris (1972)`, or just the title when the year is unknown. */
export function hitName(hit: Pick<ScrapeStreamHit["candidate"], "title" | "year">): string {
  return hit.year === null || hit.year <= 0 ? hit.title : `${hit.title} (${hit.year})`;
}

/**
 * The entries of a search as the list shows them. A provider's answer is a batch of entries; an
 * entry sent again under the same `id` replaces the one shown (a second provider knows the title
 * too), and `absorbs` names entries that were joined into it. The best match comes first (by rank: the
 * confidence plus how well known the title is, so of two titles of one name the famous one leads);
 * entries of equal rank keep the order they arrived in, so the list does not shuffle as
 * providers answer.
 */
export function mergeScrapeHits(
  items: readonly ProviderItem<ScrapeStreamHit>[],
): ScrapeStreamHit[] {
  const entries = new Map<string, ScrapeStreamHit>();
  for (const { item } of items) {
    for (const gone of item.absorbs) entries.delete(gone);
    entries.set(item.id, item);
  }
  return [...entries.values()].sort(
    (a, b) =>
      (b.candidate.rank ?? b.candidate.confidence ?? 0) -
      (a.candidate.rank ?? a.candidate.confidence ?? 0),
  );
}

/** Every `provider:id` pair an entry is known by, with the IDs that joined its providers. */
function hitIdentities(hit: ScrapeStreamHit): string[] {
  return [
    ...hit.ids,
    ...hit.providers.map((entry) => `${entry.provider}:${entry.providerId}`),
    `${hit.candidate.provider}:${hit.candidate.providerId}`,
  ];
}

/** Whether an entry is the title the item is already matched to. */
export function isCurrentMatch(hit: ScrapeStreamHit, currentIds: readonly string[]): boolean {
  if (currentIds.length === 0) return false;
  const known = new Set(currentIds.map((id) => id.toLowerCase()));
  return hitIdentities(hit).some((id) => known.has(id.toLowerCase()));
}

/** The library item that already holds this title, unless it is the item being scraped. */
export function otherLibraryLink(
  hit: ScrapeStreamHit,
  target: Pick<ScrapeTarget, "itemId">,
): LibraryLink | null {
  return hit.inLibrary !== null && hit.inLibrary.itemId !== target.itemId ? hit.inLibrary : null;
}

// ---- the diff ------------------------------------------------------------------------------

/** The groups of the diff, in the order they are listed. */
export type DiffGroup = "text" | "people" | "ratings-and-ids" | "artwork";

/**
 * The rows of the diff after the details arrived: a row of the second phase replaces the row of
 * the first with the same `field`, in place, so a row does not jump when its details come in.
 */
export function mergeDiffRows(items: readonly ProviderItem<ScrapeFieldDiff>[]): ScrapeFieldDiff[] {
  const rows = new Map<string, ScrapeFieldDiff>();
  for (const { item } of items) rows.set(item.field, item);
  return [...rows.values()];
}

export interface ApplyCounts {
  fields: number;
  images: number;
}
