import { X } from "lucide-react";

import { Button } from "../../components/Button";
import { Select } from "../../components/Select";
import { t } from "../../i18n/catalog";
import { languageName } from "../edit/names";
import { applyOriginalTitleLanguages } from "../title/titleChoice";
import { FormRow } from "./FormParts";
import { useSetting, useSettingsStore } from "./settingsStore";

/** The setting: two-letter codes of the original languages whose titles keep their original title. */
export const ORIGINAL_TITLE_LANGUAGES_KEY = "metadata.original-title-languages";

/**
 * The original languages a person can pick, as the providers give them (two-letter codes). The
 * list holds the languages most films and shows are made in; names come from the runtime.
 */
const CODES: readonly string[] = [
  "ar",
  "bn",
  "ca",
  "cs",
  "da",
  "de",
  "el",
  "en",
  "es",
  "et",
  "fa",
  "fi",
  "fr",
  "he",
  "hi",
  "hu",
  "id",
  "is",
  "it",
  "ja",
  "ko",
  "lt",
  "lv",
  "ml",
  "ms",
  "nl",
  "no",
  "pl",
  "pt",
  "ro",
  "ru",
  "sk",
  "sr",
  "sv",
  "ta",
  "te",
  "th",
  "tl",
  "tr",
  "uk",
  "vi",
  "zh",
];

/** `"fr, es"` as `["fr", "es"]`: lower-case codes, without blanks or repeats. */
function parseLanguages(text: string): string[] {
  const out: string[] = [];
  for (const part of text.split(/[\s,;]+/)) {
    const code = part.trim().toLowerCase();
    if (code !== "" && !out.includes(code)) out.push(code);
  }
  return out;
}

/**
 * Settings › Providers, "Keep the original title" beside the metadata language: the original
 * languages whose movies and shows use their original title from their first Scrape, as removable
 * pills and a list to add one, and "Use for titles I have" for the titles already in the library.
 */
export function OriginalTitleLanguages() {
  const stored = useSetting<string>(ORIGINAL_TITLE_LANGUAGES_KEY);
  const codes = parseLanguages(stored);
  const save = (next: readonly string[]) => {
    useSettingsStore.getState().set(ORIGINAL_TITLE_LANGUAGES_KEY, next.join(","));
  };
  const choices = CODES.filter((code) => !codes.includes(code))
    .map((code) => ({ value: code, label: languageName(code) }))
    .sort((a, b) => a.label.localeCompare(b.label, "en"));
  return (
    <FormRow
      label={t("settingsPage.providers.originalTitles")}
      hint={t("settingsPage.providers.originalTitles.hint")}
      labelId="original-title-languages"
      align="start"
    >
      <div className="flex flex-col gap-2">
        {codes.length === 0 ? null : (
          <ul
            aria-label={t("settingsPage.providers.originalTitles.list")}
            className="m-0 flex list-none flex-wrap gap-1.5 p-0"
          >
            {codes.map((code) => (
              <li
                key={code}
                className="inline-flex h-6 items-center gap-1 rounded-(--r-pill) bg-surface-active pr-1 pl-2.5 text-(length:--fs-12)"
              >
                {languageName(code)}
                <button
                  type="button"
                  aria-label={t("settingsPage.providers.originalTitles.remove", {
                    language: languageName(code),
                  })}
                  onClick={() => save(codes.filter((other) => other !== code))}
                  className="cv-hoverable inline-flex size-4 items-center justify-center rounded-(--r-pill) bg-transparent text-text-muted hover:text-text"
                >
                  <X size={12} strokeWidth={1.75} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <Select
            className="w-[220px]"
            aria-label={t("settingsPage.providers.originalTitles.add")}
            options={[
              { value: "", label: t("settingsPage.providers.originalTitles.add") },
              ...choices,
            ]}
            value=""
            onValueChange={(code) => {
              if (code !== "") save([...codes, code]);
            }}
          />
          <Button
            size="sm"
            disabled={codes.length === 0}
            onClick={() => void applyOriginalTitleLanguages()}
          >
            {t("settingsPage.providers.originalTitles.apply")}
          </Button>
        </div>
      </div>
    </FormRow>
  );
}
