import { t } from "../../i18n/catalog";
import type { ProviderItem, ProviderStatus } from "../workspaces/providerEvents";
import type { SourceInfo, TitleHit, TorrentRelease } from "./torrentBackend";

/** The query key of a confirmed title's details. */
export function titleQueryKey(title: { kind: string; tmdbId: number }): readonly unknown[] {
  return ["torrents", "title", title.kind, title.tmdbId];
}

// ---- titles ------------------------------------------------------------------------------

/**
 * The hits of a title search as the list shows them. A hit sent again under the same `id`
 * replaces the one shown (its poster arrived); the best match comes first, and hits of equal fit
 * keep the order they arrived in, so the list does not shuffle as providers answer.
 */
export function mergeTitleHits(items: readonly ProviderItem<TitleHit>[]): TitleHit[] {
  const entries = new Map<string, TitleHit>();
  for (const { item } of items) entries.set(item.id, item);
  return [...entries.values()].sort((a, b) => (b.confidence ?? 0) - (a.confidence ?? 0));
}

/** `Heat (1995)`, or just the title when the year is unknown. */
export function titleName(hit: Pick<TitleHit, "title" | "year">): string {
  return hit.year === null || hit.year <= 0 ? hit.title : `${hit.title} (${hit.year})`;
}

// ---- releases ----------------------------------------------------------------------------

/**
 * The releases of a search as the list holds them. A release sent again under the same `id`
 * replaces the one held (another source returned it too, or its count changed); `absorbs` names
 * rows that were folded into it and are dropped. Rows keep the order they first arrived in.
 */
export function mergeReleases(items: readonly ProviderItem<TorrentRelease>[]): TorrentRelease[] {
  const entries = new Map<string, TorrentRelease>();
  for (const { item } of items) {
    for (const gone of item.absorbs) entries.delete(gone);
    entries.set(item.id, item);
  }
  return [...entries.values()];
}

/** The resolution groups, in the order they are listed. */
const RESOLUTIONS = ["2160p", "1080p", "720p", "480p", "unknown"] as const;
export type Resolution = (typeof RESOLUTIONS)[number];

export function resolutionOf(release: Pick<TorrentRelease, "resolution">): Resolution {
  return (RESOLUTIONS as readonly string[]).includes(release.resolution)
    ? (release.resolution as Resolution)
    : "unknown";
}

// ---- rows --------------------------------------------------------------------------------

/** How healthy a release is by its seeders: 0 none, 1 a few (under 20), 2 some (under 100), 3 many. */
export function seedHealth(seeders: number | null): 0 | 1 | 2 | 3 {
  if (seeders === null || seeders <= 0) return 0;
  if (seeders < 20) return 1;
  if (seeders < 100) return 2;
  return 3;
}

// ---- age ---------------------------------------------------------------------------------

const DAY_MS = 86_400_000;

/** How long ago a release was published, short: `today`, `3 d`, `5 mo`, `2 y`. Empty when unknown. */
export function ageText(published: number | null, now: number = Date.now()): string {
  if (published === null || !Number.isFinite(published)) return "";
  const days = Math.floor((now - published) / DAY_MS);
  if (days < 1) return t("torrents.age.today");
  if (days < 60) return t("torrents.age.days", { count: days });
  if (days < 730) return t("torrents.age.months", { count: Math.floor(days / 30) });
  return t("torrents.age.years", { count: Math.floor(days / 365) });
}

// ---- sources -----------------------------------------------------------------------------

/** The name of a source by its id, from the start reply; the id itself when it is not listed. */
export function sourceName(id: string, sources: readonly SourceInfo[]): string {
  return sources.find((source) => source.id === id)?.name ?? id;
}

/** A source's name as a site: "1337x (indexer)" is "1337x". */
export function siteName(name: string): string {
  return name.replace(/\s*\(indexer\)\s*$/i, "");
}

/** Whether every source that was asked has failed and none gave anything: "No source answered". */
export function noSourceAnswered(
  providers: readonly ProviderStatus[],
  searching: boolean,
): boolean {
  return (
    !searching &&
    providers.length > 0 &&
    providers.every((status) => status.state === "failed" || status.state === "rejected")
  );
}
