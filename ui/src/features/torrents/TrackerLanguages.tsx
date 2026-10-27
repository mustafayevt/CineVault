import { useEffect, useMemo, useRef } from "react";

import { Button } from "../../components/Button";
import { t } from "../../i18n/catalog";
import { ROW_BLOCK } from "../settings/FormParts";
import { useFormat } from "../../i18n/useFormat";
import { useSettingsStore } from "../settings/settingsStore";
import { FilterChip } from "../workspaces/FilterChip";
import type { IndexerServiceStatus } from "./torrentBackend";
import {
  LANGUAGE_SOURCES,
  TRACKER_LANGUAGES_KEY,
  defaultTrackerLanguages,
  readTrackerLanguages,
} from "./trackerLanguageCodes";

/** The languages the public trackers of the indexer service are in, most common first. */
const OFFERED = [
  "en",
  "ru",
  "zh",
  "fr",
  "es",
  "de",
  "it",
  "pt",
  "pl",
  "tr",
  "uk",
  "bg",
  "hu",
  "nl",
  "cs",
  "el",
  "ro",
  "sv",
  "fi",
  "da",
  "no",
  "ja",
  "ko",
  "ar",
  "he",
  "fa",
  "hi",
  "th",
  "vi",
  "id",
];

/** How long after the last change the indexers are checked again, so several clicks are one check. */
const CHECK_AFTER_MS = 1_500;

interface TrackerLanguagesProps {
  /** The service as it stands: the default languages and what other languages would add. */
  status: IndexerServiceStatus | null;
  /** Called once the choice has settled, so the indexers are set up to match. */
  onChanged?: () => void;
}

/**
 * "Tracker languages" of the indexer service card: the languages whose public trackers are set
 * up. English and every language the person set elsewhere by default; any other can be switched
 * on, and each chip of another language says how many sites it would add. A line above the chips
 * says how many sites the other languages hold and turns them all on at once. A change is checked
 * right away (after a short pause), so the new sites join the search.
 */
export function TrackerLanguages({ status, onChanged }: TrackerLanguagesProps) {
  const format = useFormat();
  const values = useSettingsStore((state) => state.values);
  const stored = values[TRACKER_LANGUAGES_KEY];
  // The six values, not the whole settings map, decide the default.
  const sourceKey = JSON.stringify(LANGUAGE_SOURCES.map((key) => values[key] ?? null));
  const known = status?.defaultLanguages;
  const fallback = useMemo(() => {
    const sourceValues = JSON.parse(sourceKey) as unknown[];
    return defaultTrackerLanguages(
      Object.fromEntries(LANGUAGE_SOURCES.map((key, index) => [key, sourceValues[index]])),
      known ?? [],
    );
  }, [sourceKey, known]);
  const chosen = useMemo(() => readTrackerLanguages(stored) ?? fallback, [stored, fallback]);
  const names = useMemo(() => {
    try {
      return new Intl.DisplayNames([format.locale], { type: "language" });
    } catch {
      return null;
    }
  }, [format.locale]);
  const name = (code: string) => names?.of(code) ?? code;
  const counts = new Map((status?.otherLanguages ?? []).map((entry) => [entry.code, entry.count]));
  const offered = [
    ...OFFERED,
    ...[...counts.keys()].filter((code) => !OFFERED.includes(code)),
    ...chosen.filter((code) => !OFFERED.includes(code) && !counts.has(code)),
  ];
  const waiting = [...counts].filter(([code]) => !chosen.includes(code));
  const waitingSites = waiting.reduce((sum, [, count]) => sum + count, 0);

  const timer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );
  const save = (next: string[]) => {
    const same = next.length === fallback.length && next.every((other) => fallback.includes(other));
    useSettingsStore.getState().set(TRACKER_LANGUAGES_KEY, same ? undefined : next);
    if (onChanged === undefined) return;
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      onChanged();
    }, CHECK_AFTER_MS);
  };

  const toggle = (code: string) => {
    const next = chosen.includes(code)
      ? chosen.filter((other) => other !== code)
      : [...chosen, code];
    // At least one language stays; the last one cannot be switched off.
    if (next.length === 0) return;
    save(next);
  };

  return (
    <div id={TRACKER_LANGUAGES_KEY} className={ROW_BLOCK} data-testid="tracker-languages">
      <h4 id="tracker-languages__title" className="type-body m-0">
        {t("torrents.indexer.languages.title")}
      </h4>
      <p className="m-0 max-w-[72ch] text-(length:--fs-12) leading-[1.5] text-text-muted">
        {t("torrents.indexer.languages.description")}
      </p>
      {waitingSites > 0 ? (
        <div
          className="flex flex-wrap items-center gap-x-3 gap-y-1"
          data-testid="tracker-languages-off"
        >
          <span className="type-body text-text-2">
            {t("torrents.indexer.languages.off", { count: waitingSites })}
          </span>
          <Button
            variant="ghost"
            onClick={() => {
              save([...chosen, ...waiting.map(([code]) => code)]);
            }}
          >
            {t("torrents.indexer.languages.turnOn")}
          </Button>
        </div>
      ) : null}
      <div
        role="group"
        aria-labelledby="tracker-languages__title"
        className="flex flex-wrap gap-1.5"
      >
        {offered.map((code) => {
          const count = counts.get(code);
          return (
            <FilterChip
              key={code}
              pressed={chosen.includes(code)}
              disabled={chosen.length === 1 && chosen.includes(code)}
              label={
                count === undefined
                  ? undefined
                  : t("torrents.indexer.languages.chipCount", { name: name(code), count })
              }
              onClick={() => toggle(code)}
            >
              {name(code)}
              {count === undefined ? null : (
                <span aria-hidden="true" className="text-text-muted">
                  {format.number(count)}
                </span>
              )}
            </FilterChip>
          );
        })}
      </div>
    </div>
  );
}
