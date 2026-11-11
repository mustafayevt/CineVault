/**
 * Settings › Naming › Artwork file names (spec `settings` "Artwork file names in the Naming
 * section"): how the stored values read as a style or as custom names per artwork type, and what
 * a change writes.
 *
 * `metadata.artwork-names` holds the style (`prefixed` by default); `artwork.names.movie.<type>`
 * and `artwork.names.tv.<type>` hold the names of one type and replace the style's list for it
 * (`cv_artwork::naming`). The names are custom while any of those per-type keys is stored. The
 * choices, their example files and the style lists come from `artworkNameCatalog.ts`, which the
 * backend writes from the code that names the files, so the preview is what a scrape writes.
 */

import { ARTWORK_NAME_CATALOG } from "./artworkNameCatalog";

export interface ArtworkNameChoice {
  /** The stored choice: `FILENAME_POSTER`. */
  id: string;
  /** The name without extension, `{name}` standing for the video name: `{name}-poster`. */
  label: string;
  /** The file it gives for the example title. */
  example: string;
  /** Listed on every row; the others only while they are in use. */
  common: boolean;
}

export interface ArtworkNameType {
  /** The key after `artwork.names.`: `movie.poster`. */
  key: string;
  scope: "movie" | "tv";
  /** Only the first name is used (extra fanart): one choice instead of several. */
  single: boolean;
  choices: readonly ArtworkNameChoice[];
}

export interface ArtworkNameStyle {
  id: string;
  lists: Readonly<Record<string, readonly string[]>>;
}

export interface ArtworkNameCatalog {
  types: readonly ArtworkNameType[];
  styles: readonly ArtworkNameStyle[];
}

export const ARTWORK_STYLE_KEY = "metadata.artwork-names";
export const ARTWORK_NAMES_PREFIX = "artwork.names.";
export const DEFAULT_ARTWORK_STYLE = "prefixed";
/** The value the style control shows while per-type names are stored. */
export const CUSTOM_STYLE = "custom";

/** The styles the control always offers, in its order; the others show only while stored. */
export const OFFERED_STYLES: readonly string[] = ["prefixed", "short", "both", "jellyfin"];

const CATALOG: ArtworkNameCatalog = ARTWORK_NAME_CATALOG;

type Values = Readonly<Record<string, unknown>>;

export function artworkTypes(scope?: "movie" | "tv"): readonly ArtworkNameType[] {
  return scope === undefined ? CATALOG.types : CATALOG.types.filter((type) => type.scope === scope);
}

/**
 * The types the custom table lists: every movie type, and the TV types that offer a choice (a
 * show's banner has one name only, so it has no row).
 */
export function tableTypes(scope: "movie" | "tv"): readonly ArtworkNameType[] {
  return artworkTypes(scope).filter(
    (type) => type.choices.filter((choice) => choice.common).length > 1,
  );
}

export function settingKeyOf(type: ArtworkNameType): string {
  return `${ARTWORK_NAMES_PREFIX}${type.key}`;
}

/** Every per-type key the page knows. */
export function customKeys(): readonly string[] {
  return CATALOG.types.map(settingKeyOf);
}

function styleEntry(id: string): ArtworkNameStyle | undefined {
  return CATALOG.styles.find((style) => style.id === id);
}

/** The stored style, or the default when nothing (or something unknown) is stored. */
export function styleOf(values: Values): string {
  const stored = values[ARTWORK_STYLE_KEY];
  const id = typeof stored === "string" ? stored.trim().toLowerCase() : "";
  return styleEntry(id) === undefined ? DEFAULT_ARTWORK_STYLE : id;
}

/** True while any per-type key is stored. */
export function isCustom(values: Values): boolean {
  return customKeys().some((key) => Object.hasOwn(values, key));
}

/** The stored choices of a per-type value: an array or comma-separated text, upper case. */
function storedChoices(value: unknown): string[] | null {
  const items =
    typeof value === "string"
      ? value.split(",")
      : Array.isArray(value)
        ? value.filter((item): item is string => typeof item === "string")
        : null;
  if (items === null) return null;
  return items.map((item) => item.trim().toUpperCase()).filter((item) => item !== "");
}

/**
 * The names of a type in effect: its stored list when that names a choice the type knows (as the
 * backend reads it), else the list of the style.
 */
export function listOf(type: ArtworkNameType, values: Values): readonly string[] {
  const stored = storedChoices(values[settingKeyOf(type)]);
  if (stored !== null) {
    const known = new Set(type.choices.map((choice) => choice.id));
    const kept = [...new Set(stored.filter((id) => known.has(id)))];
    if (kept.length > 0) return kept;
  }
  return styleEntry(styleOf(values))?.lists[type.key] ?? [];
}

/** The per-type values that write a style's lists out as custom names: nothing changes yet. */
export function customFrom(values: Values): Record<string, string[]> {
  return Object.fromEntries(
    CATALOG.types.map((type) => [settingKeyOf(type), [...listOf(type, values)]]),
  );
}

/** The changes that pick a style: the style, and no per-type names any more. */
export function styleChanges(style: string): Record<string, unknown> {
  return {
    [ARTWORK_STYLE_KEY]: style,
    ...Object.fromEntries(customKeys().map((key) => [key, undefined])),
  };
}

/**
 * A list with one choice switched on or off, or `null` when that would leave the type without a
 * name. A type that uses only its first name takes the choice alone.
 */
export function toggled(
  type: ArtworkNameType,
  list: readonly string[],
  id: string,
): string[] | null {
  if (type.single) return [id];
  if (list.includes(id)) {
    const rest = list.filter((choice) => choice !== id);
    return rest.length === 0 ? null : rest;
  }
  // The new name takes its place in the order of the catalogue.
  const on = new Set([...list, id]);
  return type.choices.map((choice) => choice.id).filter((choice) => on.has(choice));
}

/** The choices a row shows: the common ones and any other that is in use. */
export function visibleChoices(
  type: ArtworkNameType,
  list: readonly string[],
): readonly ArtworkNameChoice[] {
  return type.choices.filter((choice) => choice.common || list.includes(choice.id));
}

/** The files a list gives for the example title, in order (only the first for a single type). */
export function examplesOf(type: ArtworkNameType, list: readonly string[]): string[] {
  const ids = type.single ? list.slice(0, 1) : list;
  const files: string[] = [];
  for (const id of ids) {
    const example = type.choices.find((choice) => choice.id === id)?.example;
    if (example !== undefined && !files.includes(example)) files.push(example);
  }
  return files;
}

/** The type of a key, for the preview of a style. */
export function artworkType(key: string): ArtworkNameType | undefined {
  return CATALOG.types.find((type) => type.key === key);
}
