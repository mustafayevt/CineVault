import { useSetting, useSettingsStore } from "../settings/settingsStore";
import { DEFAULT_PREFERRED_SOURCE, ratingSource } from "./ratingSources";

/** The setting that holds the preferred rating source (`ratings.preferred-source`). */
const PREFERRED_SOURCE_SETTING = "ratings.preferred-source";

/** A stored value as a usable source key: anything unknown gives the default. */
export function normalisePreferredSource(value: unknown): string {
  if (typeof value !== "string") return DEFAULT_PREFERRED_SOURCE;
  const key = value.trim().toLowerCase();
  return ratingSource(key) === null ? DEFAULT_PREFERRED_SOURCE : key;
}

/** The preferred rating source right now, for code outside a component. */
export function currentPreferredSource(): string {
  return normalisePreferredSource(useSettingsStore.getState().values[PREFERRED_SOURCE_SETTING]);
}

/**
 * The preferred rating source, live. The rating column, the rating sort and the `rating:` filter
 * read it, and a title without a rating from it shows the next available source, marked.
 */
export function usePreferredSource(): string {
  return normalisePreferredSource(useSetting<string>(PREFERRED_SOURCE_SETTING));
}
