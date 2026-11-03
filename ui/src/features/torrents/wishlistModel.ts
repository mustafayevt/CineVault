import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";
import type { SizeBase } from "../../i18n/locale";
import type { LibraryCopy } from "./ownedCopy";
import type { WishCriteria, WishEntry, WishMatch, WishScope } from "./torrentBackend";
import type { TitleKind } from "./torrentsSearch";

/** The lowest resolutions the editor offers; `any` takes every one. */
export const RESOLUTIONS = ["any", "720p", "1080p", "2160p"] as const;
export type ResolutionChoice = (typeof RESOLUTIONS)[number];

/** The sources the editor offers, as the release search names them. */
export const SOURCES = ["Remux", "BluRay", "WEB-DL", "WEBRip"] as const;
export type SourceChoice = (typeof SOURCES)[number];

/** What a show entry looks for. */
export type ScopeChoice = "next" | "season" | "episode";

/** Bytes in the size unit the editor uses: a GiB in base 2, a GB in base 10. */
export function sizeUnitBytes(base: SizeBase): number {
  return base === "base2" ? 1024 ** 3 : 1_000_000_000;
}

/** The editor's fields, as typed: the size and seeders stay text until saved. */
export interface WishDraft {
  scope: ScopeChoice | "movie";
  season: number;
  episode: number;
  resolution: ResolutionChoice;
  sources: readonly SourceChoice[];
  hdr: boolean;
  /** In the size unit (`sizeUnitBytes`), as typed; empty is no limit. */
  maxSizeGb: string;
  /** As typed; empty is no limit. */
  minSeeders: string;
}

/** The words of a source on screen: `Blu-ray` for `BluRay`, the others as they are. */
export function sourceLabel(source: string): string {
  return source === "BluRay" ? t("wishlist.source.bluray") : source;
}

/**
 * The criteria a new entry starts from: 1080p, or for a movie the library holds, one step above
 * its best copy (720p to 1080p, 1080p to 2160p); a 2160p copy asks for HDR, or with HDR for a
 * Remux.
 */
export function defaultCriteria(best: LibraryCopy | null): WishCriteria {
  const criteria: WishCriteria = {
    minResolution: "1080p",
    sources: [],
    hdr: false,
    maxSize: null,
    minSeeders: null,
  };
  if (best === null) return criteria;
  switch (best.resolution) {
    case "2160p":
      return best.hdr === null
        ? { ...criteria, minResolution: "2160p", hdr: true }
        : { ...criteria, minResolution: "2160p", hdr: true, sources: ["Remux"] };
    case "1080p":
      return { ...criteria, minResolution: "2160p" };
    default:
      return criteria;
  }
}

/** The scope of a new entry: a movie, or for a show the season or episode asked for, else the next new episode. */
export function defaultScope(
  kind: TitleKind,
  season: number | undefined,
  episode: number | undefined,
): WishScope {
  if (kind === "movie") return { kind: "movie", season: null, episode: null };
  if (season !== undefined && episode !== undefined) {
    return { kind: "episode", season, episode };
  }
  if (season !== undefined) return { kind: "season", season, episode: null };
  return { kind: "next", season: null, episode: null };
}

function wholeText(value: number | null, scale: number): string {
  if (value === null) return "";
  const scaled = value / scale;
  return Number.isInteger(scaled) ? String(scaled) : String(Math.round(scaled * 10) / 10);
}

/** The editor's fields for a scope and criteria; `unit` is the bytes of the size unit. */
export function draftOf(scope: WishScope, criteria: WishCriteria, unit: number): WishDraft {
  const resolution = (RESOLUTIONS as readonly string[]).includes(criteria.minResolution ?? "")
    ? (criteria.minResolution as ResolutionChoice)
    : "any";
  return {
    scope:
      scope.kind === "season" || scope.kind === "episode" || scope.kind === "next"
        ? scope.kind
        : "movie",
    season: scope.season ?? 1,
    episode: scope.episode ?? 1,
    resolution,
    sources: SOURCES.filter((source) => criteria.sources.includes(source)),
    hdr: criteria.hdr,
    maxSizeGb: wholeText(criteria.maxSize, unit),
    minSeeders: wholeText(criteria.minSeeders, 1),
  };
}

/** A typed number that must be positive; `null` for empty, `NaN` for anything else. */
function positive(text: string): number | null {
  const trimmed = text.trim().replace(",", ".");
  if (trimmed === "") return null;
  const value = Number(trimmed);
  return Number.isFinite(value) && value > 0 ? value : Number.NaN;
}

/** Why the editor cannot save, or `null` when it can. */
export function draftProblem(draft: WishDraft): string | null {
  if (Number.isNaN(positive(draft.maxSizeGb))) return t("wishlist.edit.badSize");
  const seeders = positive(draft.minSeeders);
  if (Number.isNaN(seeders) || (seeders !== null && !Number.isInteger(seeders))) {
    return t("wishlist.edit.badSeeders");
  }
  return null;
}

/** The scope and criteria the editor's fields say. Call only when [`draftProblem`] is `null`. */
export function draftValue(
  draft: WishDraft,
  unit: number,
): { scope: WishScope; criteria: WishCriteria } {
  const size = positive(draft.maxSizeGb);
  const seeders = positive(draft.minSeeders);
  const scope: WishScope =
    draft.scope === "season"
      ? { kind: "season", season: draft.season, episode: null }
      : draft.scope === "episode"
        ? { kind: "episode", season: draft.season, episode: draft.episode }
        : { kind: draft.scope, season: null, episode: null };
  return {
    scope,
    criteria: {
      minResolution: draft.resolution === "any" ? null : draft.resolution,
      sources: [...draft.sources],
      hdr: draft.hdr,
      maxSize: size === null || Number.isNaN(size) ? null : Math.round(size * unit),
      minSeeders: seeders === null || Number.isNaN(seeders) ? null : seeders,
    },
  };
}

function code(season: number, episode: number): string {
  return `S${String(season).padStart(2, "0")}E${String(episode).padStart(2, "0")}`;
}

/** What an entry looks for: "Movie", "Season 2", "S02E05", "Next new episode · S03E06". */
export function scopeText(entry: Pick<WishEntry, "scope" | "target">): string {
  const { scope, target } = entry;
  switch (scope.kind) {
    case "season":
      return t("wishlist.scope.season", { season: scope.season ?? 0 });
    case "episode":
      return code(scope.season ?? 0, scope.episode ?? 0);
    case "next":
      return target === null
        ? t("wishlist.scope.next")
        : `${t("wishlist.scope.next")} · ${code(target.season, target.episode)}`;
    default:
      return t("torrents.title.movie");
  }
}

/** The criteria in one line: "2160p or better · Remux · HDR · up to 80 GB · 10+ seeders". */
export function criteriaText(criteria: WishCriteria, format: Formatters): string {
  const parts: string[] = [
    criteria.minResolution === null
      ? t("wishlist.criteria.anyResolution")
      : t("wishlist.criteria.resolution", { resolution: criteria.minResolution }),
  ];
  if (criteria.sources.length > 0) parts.push(criteria.sources.map(sourceLabel).join(", "));
  if (criteria.hdr) parts.push(t("wishlist.criteria.hdr"));
  if (criteria.maxSize !== null) {
    parts.push(t("wishlist.criteria.maxSize", { size: format.size(criteria.maxSize) }));
  }
  if (criteria.minSeeders !== null) {
    parts.push(t("wishlist.criteria.minSeeders", { count: criteria.minSeeders }));
  }
  return parts.join(" · ");
}

/** The status line of an entry that has no match to list. */
export function noteText(entry: WishEntry, format: Formatters): string | null {
  switch (entry.note) {
    case "no-source":
      return t("wishlist.note.noSource");
    case "no-new-episode":
      return t("wishlist.note.noNewEpisode");
    case "airs": {
      const day = entry.noteDetail === null ? "" : format.date(`${entry.noteDetail}T12:00:00Z`);
      return t("wishlist.note.airs", { date: day === "" ? (entry.noteDetail ?? "") : day });
    }
    case "failed":
      return t("wishlist.note.failed", { reason: entry.noteDetail ?? "" });
    default:
      return null;
  }
}

/** The matches to show: the ones not dismissed, most seeders first (the backend's order). */
export function shownMatches(entry: WishEntry): readonly WishMatch[] {
  return entry.matches;
}

/** The entries in the order the Wishlist lists them: the newest added first, done ones last. */
export function orderEntries(entries: readonly WishEntry[]): {
  active: WishEntry[];
  done: WishEntry[];
} {
  const newest = (a: WishEntry, b: WishEntry) => (b.added ?? 0) - (a.added ?? 0) || b.id - a.id;
  return {
    active: entries.filter((entry) => entry.done === null).sort(newest),
    done: entries.filter((entry) => entry.done !== null).sort(newest),
  };
}

/** The latest check of any entry that is not done, or `null` when none was checked. */
export function lastChecked(entries: readonly WishEntry[]): number | null {
  let latest: number | null = null;
  for (const entry of entries) {
    if (entry.done !== null || entry.checked === null) continue;
    latest = latest === null ? entry.checked : Math.max(latest, entry.checked);
  }
  return latest;
}

/** Whether an entry with this title and scope is on the Wishlist (any scope when none is given). */
export function onWishlist(
  entries: readonly WishEntry[],
  kind: TitleKind,
  tmdbId: number,
): WishEntry | null {
  return entries.find((entry) => entry.kind === kind && entry.tmdbId === tmdbId) ?? null;
}
