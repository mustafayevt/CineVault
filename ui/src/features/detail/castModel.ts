import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import type { DetailCredit } from "./detailBackend";

/** The departments of the crew, in the order the tab lists them after the cast. */
const DEPARTMENTS: readonly { kinds: readonly string[]; label: MessageKey }[] = [
  { kinds: ["director"], label: "detail.cast.dept.directing" },
  { kinds: ["writer"], label: "detail.cast.dept.writing" },
  { kinds: ["producer"], label: "detail.cast.dept.production" },
  { kinds: ["camera"], label: "detail.cast.dept.camera" },
  { kinds: ["editor"], label: "detail.cast.dept.editing" },
  { kinds: ["composer"], label: "detail.cast.dept.music" },
  { kinds: ["other"], label: "detail.cast.dept.crew" },
];

export const CAST_KINDS = new Set(["actor", "guest"]);

export interface Group {
  key: string;
  label: string;
  people: DetailCredit[];
}

/** Cast first (in billing order), then each crew department the data has. */
export function castGroups(credits: readonly DetailCredit[]): Group[] {
  const groups: Group[] = [];
  const cast = credits.filter((credit) => CAST_KINDS.has(credit.kind));
  if (cast.length > 0) groups.push({ key: "cast", label: t("detail.cast.list"), people: cast });
  const known = new Set(DEPARTMENTS.flatMap((dept) => dept.kinds));
  for (const dept of DEPARTMENTS) {
    const people = credits.filter(
      (credit) =>
        dept.kinds.includes(credit.kind) ||
        (dept.kinds.includes("other") && !CAST_KINDS.has(credit.kind) && !known.has(credit.kind)),
    );
    if (people.length > 0)
      groups.push({ key: dept.kinds[0] ?? "crew", label: t(dept.label), people });
  }
  return groups;
}

/** The `nm…` id of a person's IMDb page, when one is stored. */
export function imdbPersonId(person: DetailCredit): string | null {
  const id = person.imdb_id ?? null;
  return id !== null && /^nm\d+$/.test(id) ? id : null;
}

/** The person's TMDB id, when one is stored. */
export function tmdbPersonId(person: DetailCredit): string | null {
  const id = (person.tmdb_id ?? "").trim();
  return id === "" ? null : id;
}

/** The person's page on IMDb. */
export function imdbPersonUrl(id: string): string {
  return `https://www.imdb.com/name/${encodeURIComponent(id)}/`;
}

/** The person's page on TMDB. */
export function tmdbPersonUrl(id: string): string {
  return `https://www.themoviedb.org/person/${encodeURIComponent(id)}`;
}

/**
 * Crew listed once per person: the same name under two parts (a director who also wrote) becomes
 * one entry whose role lists both, in the order they came (QA-34).
 */
export function mergeCrew(
  credits: readonly DetailCredit[],
  jobOf: (credit: DetailCredit) => string,
): DetailCredit[] {
  const merged = new Map<string, { credit: DetailCredit; jobs: string[] }>();
  for (const credit of credits) {
    const key = credit.name.trim().toLowerCase();
    const job = jobOf(credit);
    const known = merged.get(key);
    if (known === undefined) merged.set(key, { credit, jobs: [job] });
    else if (!known.jobs.includes(job)) known.jobs.push(job);
  }
  return [...merged.values()].map(({ credit, jobs }) => ({ ...credit, role: jobs.join(", ") }));
}
