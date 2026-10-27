/** The setting that holds the tracker languages of the indexer service (read by the app too). */
export const TRACKER_LANGUAGES_KEY = "torrents.indexer-service.languages";

/**
 * The settings whose languages are tracker languages by default, as the app reads them: what the
 * person reads metadata and NFO files in.
 */
export const LANGUAGE_SOURCES = [
  "metadata.language",
  "metadata.fallback-language",
  "metadata.nfo-language",
] as const;

/** Three-letter codes (both ISO 639-2 forms) of the languages trackers are in, by two-letter code. */
const THREE_LETTER: Readonly<Record<string, string>> = {
  eng: "en",
  rus: "ru",
  chi: "zh",
  zho: "zh",
  fre: "fr",
  fra: "fr",
  spa: "es",
  ger: "de",
  deu: "de",
  ita: "it",
  por: "pt",
  pol: "pl",
  tur: "tr",
  ukr: "uk",
  bul: "bg",
  hun: "hu",
  dut: "nl",
  nld: "nl",
  cze: "cs",
  ces: "cs",
  gre: "el",
  ell: "el",
  rum: "ro",
  ron: "ro",
  swe: "sv",
  fin: "fi",
  dan: "da",
  nor: "no",
  nob: "nb",
  jpn: "ja",
  kor: "ko",
  ara: "ar",
  heb: "he",
  per: "fa",
  fas: "fa",
  hin: "hi",
  tha: "th",
  vie: "vi",
  ind: "id",
  slv: "sl",
  srp: "sr",
  hrv: "hr",
  lit: "lt",
  lav: "lv",
  ice: "is",
  isl: "is",
  mac: "mk",
  mkd: "mk",
};

/**
 * The language part of a code as the indexer service names a site's language: `pt` for `pt-BR`
 * and for `por`, lower case; empty when it is not one.
 */
function languagePart(code: string): string {
  const part = code.trim().split(/[-_]/)[0] ?? "";
  if (!/^[a-z]{2,3}$/i.test(part)) return "";
  const lower = part.toLowerCase();
  return THREE_LETTER[lower] ?? lower;
}

/** The codes in a setting value: one text ("pt-BR", "en, tr; de") or a list of texts. */
function codesIn(value: unknown): string[] {
  const texts = Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : typeof value === "string"
      ? [value]
      : [];
  return texts
    .flatMap((text) => text.split(/[,;\s]+/))
    .map(languagePart)
    .filter((code) => code !== "");
}

/**
 * The tracker languages when none are chosen, as the app reads them: English with every language
 * set in [`LANGUAGE_SOURCES`], English first and the others by code. `known` adds the ones the
 * app reported (it reads the same settings, so this only fills in a code this list cannot map).
 */
export function defaultTrackerLanguages(
  values: Readonly<Record<string, unknown>>,
  known: readonly string[] = [],
): string[] {
  const others = new Set(
    [...LANGUAGE_SOURCES.flatMap((key) => codesIn(values[key])), ...known].filter(
      (code) => code !== "en" && code !== "",
    ),
  );
  return ["en", ...[...others].sort()];
}

/** The chosen tracker languages in a setting value, or `null` when none are chosen. */
export function readTrackerLanguages(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  const codes = [
    ...new Set(
      value
        .filter((code): code is string => typeof code === "string")
        .map(languagePart)
        .filter((code) => code !== ""),
    ),
  ];
  return codes.length === 0 ? null : codes;
}
