/**
 * Countries and languages by name. The library may hold a code (`US`, `en`) or a name
 * (`United States`, `English`) in these fields; people always read the name, and the code is
 * a Technical-level detail.
 */

function displayNames(type: "region" | "language"): Intl.DisplayNames | null {
  try {
    return new Intl.DisplayNames(["en"], { type, fallback: "none" });
  } catch {
    return null;
  }
}

const regions = displayNames("region");
const languages = displayNames("language");

function lookup(names: Intl.DisplayNames | null, code: string): string | undefined {
  if (names === null) return undefined;
  try {
    return names.of(code) ?? undefined;
  } catch {
    return undefined;
  }
}

/** The name of a country code, or the text itself when it is not a code (it is already a name). */
export function countryName(value: string): string {
  const trimmed = value.trim();
  if (/^[A-Za-z]{2}$/.test(trimmed)) return lookup(regions, trimmed.toUpperCase()) ?? trimmed;
  return trimmed;
}

/** The name of a language code (`en`, `eng`), or the text itself when it is already a name. */
export function languageName(value: string): string {
  const trimmed = value.trim();
  if (/^[A-Za-z]{2,3}$/.test(trimmed)) return lookup(languages, trimmed.toLowerCase()) ?? trimmed;
  return trimmed;
}

const LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

function allCodes(): string[] {
  const codes: string[] = [];
  for (const a of LETTERS) for (const b of LETTERS) codes.push(`${a}${b}`);
  return codes;
}

let countryNames: readonly string[] | null = null;
let languageNames: readonly string[] | null = null;

/** Every country name the runtime knows, sorted, for the suggestions of the field. */
export function allCountryNames(): readonly string[] {
  countryNames ??= [
    ...new Set(
      allCodes()
        .map((code) => lookup(regions, code))
        .filter((name): name is string => name !== undefined),
    ),
  ].sort((a, b) => a.localeCompare(b, "en"));
  return countryNames;
}

/** Every language name the runtime knows for a two-letter code, sorted. */
export function allLanguageNames(): readonly string[] {
  languageNames ??= [
    ...new Set(
      allCodes()
        .map((code) => lookup(languages, code.toLowerCase()))
        .filter((name): name is string => name !== undefined),
    ),
  ].sort((a, b) => a.localeCompare(b, "en"));
  return languageNames;
}

/**
 * What to store for a country the user typed: its name when the text is a code or a known name
 * in any case, else the text as typed.
 */
export function resolveCountry(typed: string): string {
  const text = typed.trim();
  const byCode = countryName(text);
  if (byCode !== text) return byCode;
  return allCountryNames().find((name) => name.toLowerCase() === text.toLowerCase()) ?? text;
}

/** What to store for a language the user typed; see {@link resolveCountry}. */
export function resolveLanguage(typed: string): string {
  const text = typed.trim();
  const byCode = languageName(text);
  if (byCode !== text) return byCode;
  return allLanguageNames().find((name) => name.toLowerCase() === text.toLowerCase()) ?? text;
}
