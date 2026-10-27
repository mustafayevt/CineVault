import { t } from "../../i18n/catalog";
import { ratingSourceName } from "../ratings/ratingSources";
import type { EditField, EditValue } from "./editBackend";

/** Builders and readers for the values the editor passes around. */
export const value = {
  text: (text: string): EditValue => ({ type: "text", value: text }),
  number: (n: number | null): EditValue => ({ type: "number", value: n }),
  flag: (on: boolean): EditValue => ({ type: "flag", value: on }),
  date: (date: string | null): EditValue => ({ type: "date", value: date }),
  list: (values: readonly string[]): EditValue => ({ type: "list", values: [...values] }),
};

export function asText(v: EditValue | undefined): string {
  return v?.type === "text" ? v.value : "";
}

export function asNumber(v: EditValue | undefined): number | null {
  return v?.type === "number" ? v.value : null;
}

export function asFlag(v: EditValue | undefined): boolean {
  return v?.type === "flag" ? v.value : false;
}

export function asDate(v: EditValue | undefined): string {
  return v?.type === "date" ? (v.value ?? "") : "";
}

export function asList(v: EditValue | undefined): readonly string[] {
  return v?.type === "list" ? v.values : [];
}

/** Whether two values say the same. The values are plain data, so comparing their JSON is enough. */
export function sameValue(a: EditValue | undefined, b: EditValue | undefined): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Words of stored names that stay in capitals: `UHD_BLURAY` reads "UHD Bluray". */
const ABBREVIATIONS: ReadonlySet<string> = new Set([
  "UHD",
  "HD",
  "HDDVD",
  "DVD",
  "DVDSCR",
  "TV",
  "VHS",
  "DL",
  "R5",
  "TS",
  "TC",
  "IMAX",
  "HDRIP",
  "WEBRIP",
]);

/** The stored name of a genre, edition or source (`SCIENCE_FICTION`) as people write it. */
export function prettyName(name: string): string {
  if (!/^[A-Z0-9_]+$/.test(name)) return name;
  return name
    .split("_")
    .map((word) => (ABBREVIATIONS.has(word) ? word : word.charAt(0) + word.slice(1).toLowerCase()))
    .join(" ");
}

/** The range a number field accepts; the controls use the same bounds. */
export interface NumberRule {
  min: number;
  max: number;
  /** The values go in these steps from `min` (1 for whole numbers). */
  step: number;
}

/** The years the editors accept; empty means unknown. The library refuses anything else. */
export const YEAR_RULE: NumberRule = { min: 1800, max: 2200, step: 1 };

/** The number fields with a range, as the library checks them. */
export const NUMBER_RULES: Readonly<Record<string, NumberRule>> = {
  year: YEAR_RULE,
  runtime: { min: 0, max: 100_000, step: 1 },
  top250: { min: 0, max: 250, step: 1 },
  user_rating: { min: 0.5, max: 10, step: 0.5 },
};

const NUMBER_MESSAGES = {
  year: "edit.invalid.year",
  runtime: "edit.invalid.runtime",
  top250: "edit.invalid.top250",
  user_rating: "edit.invalid.userRating",
} as const;

function fits(n: number, rule: NumberRule): boolean {
  if (!Number.isFinite(n) || n < rule.min || n > rule.max) return false;
  const steps = (n - rule.min) / rule.step;
  return Math.abs(steps - Math.round(steps)) < 1e-9;
}

/**
 * The value as it would be saved: a rating row left as it came (an "Add rating" that was not
 * filled in) is no rating, so it is neither a change nor sent.
 */
export function meaningful(v: EditValue): EditValue {
  if (v.type === "ratings") {
    // An "Add rating" row left as it came (no source, no value, no votes) is no rating.
    return {
      type: "ratings",
      ratings: v.ratings.filter(
        (row) => row.source.trim() !== "" || row.value !== 0 || row.votes !== 0,
      ),
    };
  }
  return v;
}

/**
 * Why a value cannot be saved, in a sentence for the line under the field, or `null` when it is
 * fine. The library checks the same, so a value the form lets through is not refused later.
 */
export function problemOf(key: string, v: EditValue | undefined): string | null {
  if (key === "title" && v?.type === "text" && v.value.trim() === "") {
    return t("edit.invalid.title");
  }
  if (v?.type === "ratings") return ratingsProblem(v);
  const rule = NUMBER_RULES[key];
  if (rule === undefined || v?.type !== "number" || v.value === null) return null;
  // A zero year is "unknown", like an empty one.
  if (key === "year" && v.value === 0) return null;
  if (fits(v.value, rule)) return null;
  const message = NUMBER_MESSAGES[key as keyof typeof NUMBER_MESSAGES];
  // As text: a year reads 1800, not 1,800.
  return t(message, { min: String(rule.min), max: String(rule.max) });
}

/** Why a ratings list cannot be saved; the library checks the same. */
function ratingsProblem(v: Extract<EditValue, { type: "ratings" }>): string | null {
  const kept = meaningful(v);
  const rows = kept.type === "ratings" ? kept.ratings : [];
  const seen = new Set<string>();
  for (const row of rows) {
    const key = row.source.trim().toLowerCase();
    if (key === "") return t("edit.invalid.ratingSource");
    if (key === "user") return t("edit.invalid.ratingUser");
    const source = ratingSourceName(key);
    if (seen.has(key)) return t("edit.invalid.ratingTwice", { source });
    seen.add(key);
    if (!Number.isInteger(row.max) || row.max < 1) {
      return t("edit.invalid.ratingScale", { source });
    }
    if (row.value === null || !Number.isFinite(row.value) || row.value < 0 || row.value > row.max) {
      return t("edit.invalid.ratingRange", { source, max: String(row.max) });
    }
    if (!Number.isInteger(row.votes) || row.votes < 0) {
      return t("edit.invalid.ratingVotes", { source });
    }
  }
  return null;
}

/** The problems of the values, by key; empty when all can be saved. */
export function problemsOf(
  values: Readonly<Record<string, EditValue | undefined>>,
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, v] of Object.entries(values)) {
    const problem = problemOf(key, v);
    if (problem !== null) out[key] = problem;
  }
  return out;
}

/** The fields by key. */
export function indexFields(fields: readonly EditField[]): ReadonlyMap<string, EditField> {
  return new Map(fields.map((field) => [field.key, field]));
}

/** The keys of the season fields of a show: `season_name.2` and `season_overview.2`. */
export const seasonNameKey = (season: number): string => `season_name.${season}`;
export const seasonOverviewKey = (season: number): string => `season_overview.${season}`;

/** Whether a value says nothing: what a field that was never stored reads as. */
export function isBlank(v: EditValue | undefined): boolean {
  if (v === undefined) return true;
  switch (v.type) {
    case "text":
      return v.value === "";
    case "number":
    case "date":
      return v.value === null;
    case "flag":
      return !v.value;
    case "list":
      return v.values.length === 0;
    case "pairs":
      return v.pairs.length === 0;
    case "people":
      return v.people.length === 0;
    case "trailers":
      return v.trailers.length === 0;
    case "numbers":
      return v.numbers.length === 0;
    case "ratings":
      return v.ratings.length === 0;
  }
}
