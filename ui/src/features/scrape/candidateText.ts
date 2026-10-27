import { t } from "../../i18n/catalog";
import type { ReviewCandidateDto } from "../../bindings";
import { providerIdLabel, providerIdUrl } from "../edit/providerIds";
import { providerName } from "../workspaces/providerNames";
import type { ScrapeKind, ScrapeStreamHit } from "./scrapeBackend";

/** What the search knows about the file, to say why a candidate scored as it did. */
export interface LocalFacts {
  year: number | null;
  runtimeMinutes: number | null;
}

/** "2h 56m", "56m": a runtime in minutes in the short form the pages use. */
export function runtimeText(minutes: number | null): string | null {
  if (minutes === null || minutes <= 0) return null;
  const hours = Math.floor(minutes / 60);
  const rest = Math.round(minutes % 60);
  if (hours === 0) return `${rest}m`;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}

/** The file's length in whole minutes, or `null` when it was not read. */
export function minutesOf(seconds: number | null): number | null {
  return seconds === null || seconds <= 0 ? null : Math.round(seconds / 60);
}

/** "2h 56m · Matt Reeves": the runtime and the director when the provider says them. */
export function metaLine(candidate: ReviewCandidateDto): string {
  return [runtimeText(candidate.runtimeMinutes), candidate.director]
    .filter((part): part is string => part !== null && part !== "")
    .join(" · ");
}

function capitalised(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * Why a candidate scored as it did, in a short line: "Title, year and runtime match" or "Title
 * matches · year differs by 9 · runtime off by 69m". Only what the file and the provider both say
 * is compared.
 */
export function whyLine(candidate: ReviewCandidateDto, local: LocalFacts): string {
  const similarity = candidate.breakdown.similarity ?? 0;
  const title =
    similarity >= 0.98
      ? "matches"
      : similarity >= 0.5
        ? "partly"
        : similarity > 0
          ? "differs"
          : null;
  const yearGap =
    local.year !== null && local.year > 0 && candidate.year !== null && candidate.year > 0
      ? Math.abs(local.year - candidate.year)
      : null;
  const runtimeGap =
    local.runtimeMinutes !== null &&
    candidate.runtimeMinutes !== null &&
    candidate.runtimeMinutes > 0
      ? Math.abs(local.runtimeMinutes - candidate.runtimeMinutes)
      : null;
  const runtimeSame =
    runtimeGap !== null && runtimeGap <= Math.max(2, (local.runtimeMinutes ?? 0) * 0.05);

  if (title === "matches" && yearGap === 0 && (runtimeSame || runtimeGap === null)) {
    return runtimeGap === null ? t("scrapePage.why.titleYearMatch") : t("scrapePage.why.allMatch");
  }
  if (title === "matches" && yearGap === null && runtimeSame) {
    return t("scrapePage.why.titleRuntimeMatch");
  }
  const parts: string[] = [];
  if (title === "matches") parts.push(t("scrapePage.why.titleMatches"));
  else if (title === "partly") parts.push(t("scrapePage.why.titlePartly"));
  if (yearGap !== null) {
    parts.push(
      yearGap === 0
        ? t("scrapePage.why.yearMatches")
        : t("scrapePage.why.yearDiffers", { years: yearGap }),
    );
  }
  if (runtimeGap !== null) {
    parts.push(
      runtimeSame
        ? t("scrapePage.why.runtimeMatches")
        : t("scrapePage.why.runtimeDiffers", { minutes: runtimeText(runtimeGap) ?? "" }),
    );
  }
  return capitalised(parts.join(" · "));
}

/** One ID of a candidate with the page it opens, for the line under the chosen title. */
export interface IdLink {
  label: string;
  value: string;
  url: string | null;
}

/** The IDs the providers gave for a candidate: "TMDB 414906", "IMDb tt1877830". */
export function idLinks(hit: ScrapeStreamHit, kind: ScrapeKind): IdLink[] {
  const found = new Map<string, string>();
  for (const entry of hit.providers) {
    const key = entry.provider === "omdb" ? "imdb" : entry.provider;
    if (!found.has(key)) found.set(key, entry.providerId);
  }
  for (const id of hit.ids) {
    const at = id.indexOf(":");
    if (at > 0) found.set(id.slice(0, at), id.slice(at + 1));
  }
  const order = ["tmdb", "imdb", "tvdb"];
  return [...found.entries()]
    .sort(([a], [b]) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99))
    .map(([source, value]) => ({
      label: providerIdLabel(source),
      value,
      url: providerIdUrl(source, value, kind),
    }));
}

/** Whether the text is a pasted link or ID that the search takes as an exact title. */
export function isLinkOrId(text: string): boolean {
  const value = text.trim();
  if (value === "") return false;
  return (
    /^(?:https?:\/\/)?(?:[a-z0-9-]+\.)*(?:imdb|themoviedb|thetvdb)\.com\//i.test(value) ||
    /^(?:tt\d{6,9}|(?:tmdb|tvdb|imdb)\s*[:=]?\s*[\w-]+)$/i.test(value)
  );
}

/** The names of the providers behind a list of candidates, for "5 results from TMDB". */
export function providersOf(search: {
  stream: { providers: readonly { provider: string; count: number }[] };
  providerNames: Readonly<Record<string, string>>;
  entries: readonly ScrapeStreamHit[];
}): string {
  const answered = search.stream.providers
    .filter((status) => status.count > 0)
    .map((status) => search.providerNames[status.provider] ?? providerName(status.provider));
  const names =
    answered.length > 0
      ? answered
      : [...new Set(search.entries.map((hit) => hit.candidate.provider))].map((key) =>
          providerName(key),
        );
  return names.join(", ");
}

/**
 * How many minutes the candidate's runtime is off from the file's, when that is more than a
 * rounding difference (the larger of 5 minutes and 5% of the file); `null` when it is not, or
 * when either runtime is unknown.
 */
export function runtimeGap(
  fileMinutes: number | null,
  candidateMinutes: number | null,
): number | null {
  if (fileMinutes === null || fileMinutes <= 0) return null;
  if (candidateMinutes === null || candidateMinutes <= 0) return null;
  const gap = Math.abs(fileMinutes - candidateMinutes);
  return gap > Math.max(5, fileMinutes * 0.05) ? gap : null;
}
