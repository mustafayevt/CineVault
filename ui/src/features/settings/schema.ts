/**
 * What the settings page shows: its sections and every setting with its default, control,
 * description and search keywords.
 *
 * Only values that differ from the default are stored (see `settingsStore.ts`), so a setting is
 * "modified" exactly when it holds a non-default value. The ids double as storage keys and as the
 * element ids the command palette jumps to (`/settings?setting=<id>`); the palette's setting
 * index is built from this list, so the two cannot drift apart.
 *
 * The words come from the string catalogue (`i18n/messages/en/*.ts`): a setting `a.b` reads
 * `settings.a.b.title`, `.description` and `.keywords`. The text fields are getters, so they are
 * looked up when read and follow the catalogue in use.
 */

import { t, tList } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { LOCALE_SYSTEM, defaultSizeBase } from "../../i18n/locale";
import type { TextFormat } from "./textFormats";
import {
  DEFAULT_PREFERRED_SOURCE,
  PREFERRABLE_SOURCES,
  ratingSourceName,
} from "../ratings/ratingSources";

export type SettingValue = string | number | boolean;

/**
 * The groups the settings were listed in before the one-page Settings; kept as each setting's tag.
 */
export type SectionId =
  "general" | "library" | "providers" | "extras" | "naming" | "advanced" | "torrents";

export interface ChoiceOption {
  value: string;
  label: string;
}

export type SettingControl =
  | { type: "toggle" }
  /** Up to three short options show as a segmented control, more as a drop-down list. */
  | { type: "choice"; options: readonly ChoiceOption[] }
  | { type: "number"; min: number; max: number; step?: number; unit?: string; slider?: boolean }
  /** `allowEmpty` lets a person clear a text whose default is not empty (a list with a built-in start). */
  | {
      type: "text";
      placeholder?: string;
      mono?: boolean;
      multiline?: boolean;
      allowEmpty?: boolean;
      /** A format the text must have; an entry that breaks it is shown and never saved. */
      format?: TextFormat;
    }
  /**
   * Several options that can each be on or off. The value is the comma-separated list of the
   * options that are on, in the order of `options`.
   */
  | {
      type: "checklist";
      options: readonly ChoiceOption[];
      /** What the options are, for the summary line ("2 of 4 sources selected"). */
      noun?: "fields" | "sources";
    };

export interface SettingDef {
  /** Storage key and element id. Lowercase words separated by dots and hyphens. */
  id: string;
  section: SectionId;
  title: string;
  /** What the setting does, in plain words. Searched too. */
  description: string;
  /** Other words people may type to find it. */
  keywords: readonly string[];
  control: SettingControl;
  /** The value when nothing is stored. A link has none. */
  default: SettingValue | null;
  /**
   * An expert setting: its group lists it under a collapsed "More options" instead of among the
   * essentials. A search and the palette still find it, and opening it expands the list.
   */
  advanced: boolean;
}

/** The ids of the settings the catalogue has text for. */
type SettingId = MessageKey extends infer K
  ? K extends `settings.${infer Id}.keywords`
    ? Id
    : never
  : never;

/**
 * The expert settings, by id. Each group keeps its essentials on show and lists these under
 * "More options": tuning that most people never touch, and values the app picks well itself.
 */
const ADVANCED_IDS: ReadonlySet<string> = new Set([
  // General
  "appearance.motion",
  "general.update-channel",
  "general.update-public-key",
  // Library and folders
  "library.skip-nomedia",
  "library.vsmeta-artwork",
  "library.bad-words",
  "library.watch-debounce-seconds",
  "media.store-mediainfo-xml",
  "media.runtime-from-media",
  "media.detect-aspect-ratio",
  "media.aspect-mode",
  "media.aspect-round-up",
  "media.aspect-multi-format",
  "media.checksums-after-scrape",
  "renamer.cleanup-unwanted",
  "renamer.unwanted-patterns",
  // Scraping and providers
  "metadata.scrape-fields-movie",
  "metadata.scrape-fields-show",
  "metadata.scrape-fields-episode",
  "ratings.sources-movie",
  "ratings.sources-show",
  "tv.season-art-fallback",
  "metadata.fallback-language",
  "providers.prefer-original-title",
  // Artwork
  "subtitles.language-style",
  "metadata.artwork-names",
  "artwork.poster-size",
  "artwork.fanart-size",
  "artwork.other-resolutions",
  "artwork.languages",
  "artwork.fanart-without-text",
  "artwork.any-language",
  "artwork.collection-locations",
  "artwork.extra-fanart-enabled",
  "artwork.extra-fanart",
  "artwork.extra-thumbs",
  "artwork.extra-thumbs-count",
  "artwork.extra-thumbs-resize",
  "artwork.extra-thumbs-width",
  "artwork.actor-images",
  // NFO files
  "metadata.nfo-movie-names",
  "metadata.nfo-disc-folder",
  "metadata.nfo-set-names",
  "metadata.nfo-collection-folder",
  "metadata.nfo-collection-tmdb-id",
  "metadata.collection-name-replacement",
  "metadata.nfo-season-names",
  "metadata.nfo-all-actors",
  "metadata.nfo-episodeguide",
  "metadata.nfo-end-date",
  "metadata.nfo-mediaportal-style",
  "metadata.nfo-certification",
  "metadata.nfo-create-outline",
  "metadata.nfo-outline-first-sentence",
  "metadata.nfo-single-studio",
  "metadata.nfo-date-added",
  "metadata.nfo-date-added-source",
  "metadata.nfo-fileinfo",
  "metadata.nfo-external-audio",
  "metadata.nfo-lockdata",
  "metadata.nfo-clean",
  // Torrent sources
  "torrents.url.torrentio",
  "torrents.url.yts",
  "torrents.url.eztv",
  "torrents.url.apibay",
  "torrents.url.knaben",
  "torrents.url.nyaa",
]);

function setting<Id extends SettingId>(
  id: Id,
  sectionId: SectionId,
  control: SettingControl,
  defaultValue: SettingValue | null,
): SettingDef {
  return {
    id,
    section: sectionId,
    get title() {
      return t(`settings.${id}.title`);
    },
    get description() {
      return t(`settings.${id}.description`);
    },
    get keywords() {
      return tList(`settings.${id}.keywords`);
    },
    control,
    default: defaultValue,
    advanced: ADVANCED_IDS.has(id),
  };
}

/** Options whose labels are catalogue messages: `settings.<id>.option.<value>`. */
function translatedOptions(id: string, values: readonly string[]): readonly ChoiceOption[] {
  return values.map((value) => ({
    value,
    get label() {
      return t(`settings.${id}.option.${value}` as MessageKey);
    },
  }));
}

/**
 * The fields a scrape may write, per kind of item, in the order the backend lists them
 * (`cv_domain::MetaField`). The labels are `scrape.field.<key>` in the catalogue.
 */
export const SCRAPE_FIELDS = {
  movie: [
    "title",
    "original_title",
    "tagline",
    "plot",
    "year",
    "release_date",
    "rating",
    "top250",
    "runtime",
    "certification",
    "genres",
    "spoken_languages",
    "country",
    "studio",
    "collection",
    "actors",
    "crew",
    "trailer",
    "poster",
    "fanart",
    "banner",
    "clearart",
    "disc",
    "clearlogo",
    "thumb",
    "keyart",
    "squareart",
    "extrafanart",
    "extrathumb",
  ],
  show: [
    "title",
    "original_title",
    "tagline",
    "plot",
    "year",
    "release_date",
    "status",
    "rating",
    "top250",
    "runtime",
    "certification",
    "genres",
    "country",
    "studio",
    "actors",
    "crew",
    "trailer",
    "poster",
    "fanart",
    "banner",
    "clearart",
    "disc",
    "clearlogo",
    "thumb",
    "characterart",
    "keyart",
    "squareart",
    "extrafanart",
  ],
  episode: [
    "title",
    "original_title",
    "plot",
    "release_date",
    "rating",
    "runtime",
    "studio",
    "actors",
    "crew",
    "thumb",
  ],
} as const;

export type ScrapeKind = keyof typeof SCRAPE_FIELDS;

/**
 * The fields a scrape writes until the user chooses: all of them. Tags are not a scrape field: a
 * provider's keywords are never imported, and the tags a title has stay as they are
 * (`scrape_settings.rs`).
 */
function defaultScrapeFields(kind: ScrapeKind): string {
  return SCRAPE_FIELDS[kind].join(",");
}

/** Checklist options for the fields of one kind: labels are `scrape.field.<key>`. */
function scrapeFieldOptions(kind: ScrapeKind): readonly ChoiceOption[] {
  return SCRAPE_FIELDS[kind].map((value) => ({
    value,
    get label() {
      return t(`scrape.field.${value}`);
    },
  }));
}

/** The sources a person can prefer for the rating column; the names are the sources' own. */
const PREFERRED_RATING_OPTIONS: readonly ChoiceOption[] = PREFERRABLE_SOURCES.map((value) => ({
  value,
  label: ratingSourceName(value),
}));

/** Where Fetch ratings looks (`cv_providers::RatingSource`); labels are `settings.ratings.source.<key>`. */
const RATING_FETCH_SOURCES: readonly ChoiceOption[] = [
  "imdb-dataset",
  "scraper",
  "omdb",
  "mdblist",
].map((value) => ({
  value,
  get label() {
    return t(`settings.ratings.source.${value}` as MessageKey);
  },
}));

/**
 * The sources asked by default (`RatingFetchSettings::DEFAULT_SOURCES`): a source whose key is not
 * set is skipped, so a key saved on the Providers page is used without choosing it here as well.
 */
const DEFAULT_RATING_FETCH_SOURCES = "imdb-dataset,omdb,mdblist";

/** The picture sizes artwork providers offer, smallest first. */
const ARTWORK_SIZES = ["small", "medium", "big", "large", "xlarge"] as const;

export const LANGUAGES: readonly ChoiceOption[] = [
  { value: "en", label: "English" },
  { value: "de", label: "Deutsch" },
  { value: "fr", label: "Français" },
  { value: "es", label: "Español" },
  { value: "it", label: "Italiano" },
  { value: "pt", label: "Português" },
  { value: "nl", label: "Nederlands" },
  { value: "sv", label: "Svenska" },
  { value: "pl", label: "Polski" },
  { value: "tr", label: "Türkçe" },
  { value: "ru", label: "Русский" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "zh", label: "中文" },
];

/** Countries whose certification and release dates a scrape may prefer, named in English. */
export const COUNTRIES: readonly ChoiceOption[] = [
  ["US", "United States"],
  ["GB", "United Kingdom"],
  ["CA", "Canada"],
  ["AU", "Australia"],
  ["DE", "Germany"],
  ["FR", "France"],
  ["ES", "Spain"],
  ["IT", "Italy"],
  ["PT", "Portugal"],
  ["BR", "Brazil"],
  ["NL", "Netherlands"],
  ["SE", "Sweden"],
  ["PL", "Poland"],
  ["TR", "Türkiye"],
  ["RU", "Russia"],
  ["JP", "Japan"],
  ["KR", "South Korea"],
  ["CN", "China"],
].map(([value, label]) => ({ value: value ?? "", label: label ?? "" }));

/** Regions for dates, times and numbers, each named in its own language. */
const REGIONS: readonly ChoiceOption[] = [
  { value: "en-US", label: "English (United States)" },
  { value: "en-GB", label: "English (United Kingdom)" },
  { value: "de-DE", label: "Deutsch (Deutschland)" },
  { value: "fr-FR", label: "Français (France)" },
  { value: "es-ES", label: "Español (España)" },
  { value: "it-IT", label: "Italiano (Italia)" },
  { value: "pt-BR", label: "Português (Brasil)" },
  { value: "nl-NL", label: "Nederlands (Nederland)" },
  { value: "sv-SE", label: "Svenska (Sverige)" },
  { value: "pl-PL", label: "Polski (Polska)" },
  { value: "tr-TR", label: "Türkçe (Türkiye)" },
  { value: "ru-RU", label: "Русский (Россия)" },
  { value: "ja-JP", label: "日本語 (日本)" },
  { value: "ko-KR", label: "한국어 (대한민국)" },
  { value: "zh-CN", label: "中文 (中国)" },
];

/** The regional-format choices: follow the system first, then each region. */
function localeOptions(): readonly ChoiceOption[] {
  return [...translatedOptions("general.locale", [LOCALE_SYSTEM]), ...REGIONS];
}

const ALL_SETTINGS: readonly SettingDef[] = [
  setting(
    "appearance.theme",
    "general",
    { type: "choice", options: translatedOptions("appearance.theme", ["system", "dark", "light"]) },
    "dark",
  ),
  setting(
    "appearance.motion",
    "general",
    {
      type: "choice",
      options: translatedOptions("appearance.motion", ["system", "reduced"]),
    },
    "system",
  ),
  setting(
    "appearance.zoom",
    "general",
    { type: "number", min: 80, max: 150, step: 5, unit: "%" },
    100,
  ),
  setting("general.locale", "general", { type: "choice", options: localeOptions() }, LOCALE_SYSTEM),
  setting("general.check-updates", "general", { type: "toggle" }, true),
  setting(
    "general.update-interval",
    "general",
    {
      type: "choice",
      options: translatedOptions("general.update-interval", ["daily", "weekly", "monthly"]),
    },
    "daily",
  ),
  setting(
    "general.update-channel",
    "general",
    {
      type: "text",
      mono: true,
      placeholder: "Built-in channel",
      format: "https-url",
    },
    "",
  ),
  setting(
    "general.update-public-key",
    "general",
    { type: "text", mono: true, placeholder: "base64", format: "signing-key" },
    "",
  ),
  setting("library.reset-new-on-update", "library", { type: "toggle" }, false),
  setting("library.remember-filters", "library", { type: "toggle" }, false),
  setting(
    "library.skip-patterns",
    "library",
    { type: "text", mono: true, allowEmpty: true, placeholder: "MAKEMKV; ^Extras$" },
    "MAKEMKV",
  ),
  setting("library.skip-nomedia", "library", { type: "toggle" }, true),
  setting("library.vsmeta-artwork", "library", { type: "toggle" }, false),
  setting(
    "library.bad-words",
    "library",
    { type: "text", mono: true, placeholder: "1080p, bluray" },
    "",
  ),
  setting(
    "library.watch-debounce-seconds",
    "library",
    { type: "number", min: 5, max: 600, step: 1, unit: "s" },
    5,
  ),
  setting("media.store-mediainfo-xml", "library", { type: "toggle" }, false),
  setting("media.runtime-from-media", "library", { type: "toggle" }, false),
  setting("media.detect-aspect-ratio", "library", { type: "toggle" }, false),
  setting(
    "media.aspect-mode",
    "library",
    {
      type: "choice",
      options: translatedOptions("media.aspect-mode", ["fast", "default", "accurate"]),
    },
    "default",
  ),
  setting("media.aspect-round-up", "library", { type: "toggle" }, false),
  setting(
    "media.aspect-multi-format",
    "library",
    {
      type: "choice",
      options: translatedOptions("media.aspect-multi-format", ["most-common", "higher", "wider"]),
    },
    "most-common",
  ),
  setting(
    "media.checksums-after-scrape",
    "library",
    {
      type: "choice",
      options: translatedOptions("media.checksums-after-scrape", ["off", "crc32", "phash", "all"]),
    },
    "off",
  ),
  setting("movies.collections.auto-group", "providers", { type: "toggle" }, true),
  setting(
    "metadata.nfo-movie-names",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-movie-names", ["video-base", "movie", "both"]),
    },
    "video-base",
  ),
  setting("metadata.nfo-disc-folder", "naming", { type: "toggle" }, false),
  setting(
    "metadata.nfo-set-names",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-set-names", [
        "set-name",
        "set",
        "collection",
        "all",
      ]),
    },
    "set-name",
  ),
  setting(
    "metadata.nfo-collection-folder",
    "naming",
    { type: "text", mono: true, placeholder: "D:\\Collections" },
    "",
  ),
  setting("metadata.nfo-collection-tmdb-id", "naming", { type: "toggle" }, false),
  setting(
    "metadata.collection-name-replacement",
    "naming",
    { type: "text", mono: true, placeholder: "_" },
    "_",
  ),
  setting(
    "tv.episode-order",
    "providers",
    {
      type: "choice",
      options: translatedOptions("tv.episode-order", ["aired", "dvd", "absolute"]),
    },
    "aired",
  ),
  setting("tv.specials.show", "providers", { type: "toggle" }, true),
  setting("tv.season-art-fallback", "providers", { type: "toggle" }, true),
  setting(
    "metadata.nfo-season-names",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-season-names", ["season", "season-folder", "both"]),
    },
    "season",
  ),
  setting("metadata.nfo-all-actors", "naming", { type: "toggle" }, false),
  setting(
    "metadata.nfo-episodeguide",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-episodeguide", ["json", "url", "off"]),
    },
    "json",
  ),
  setting("metadata.nfo-end-date", "naming", { type: "toggle" }, true),
  setting("metadata.language", "providers", { type: "choice", options: LANGUAGES }, "en"),
  setting("metadata.country", "providers", { type: "choice", options: COUNTRIES }, "US"),
  setting("metadata.local-release-date", "providers", { type: "toggle" }, false),
  setting("metadata.fallback-language", "providers", { type: "choice", options: LANGUAGES }, "en"),
  setting(
    "matching.auto-accept-threshold",
    "providers",
    { type: "number", min: 50, max: 100, step: 1, unit: "%", slider: true },
    75,
  ),
  setting("metadata.scrape-do-not-overwrite", "providers", { type: "toggle" }, false),
  setting(
    "metadata.scrape-fields-movie",
    "providers",
    { type: "checklist", options: scrapeFieldOptions("movie") },
    defaultScrapeFields("movie"),
  ),
  setting(
    "metadata.scrape-fields-show",
    "providers",
    { type: "checklist", options: scrapeFieldOptions("show") },
    defaultScrapeFields("show"),
  ),
  setting(
    "metadata.scrape-fields-episode",
    "providers",
    { type: "checklist", options: scrapeFieldOptions("episode") },
    defaultScrapeFields("episode"),
  ),
  setting(
    "ratings.preferred-source",
    "providers",
    { type: "choice", options: PREFERRED_RATING_OPTIONS },
    DEFAULT_PREFERRED_SOURCE,
  ),
  setting(
    "ratings.sources-movie",
    "providers",
    { type: "checklist", options: RATING_FETCH_SOURCES, noun: "sources" },
    DEFAULT_RATING_FETCH_SOURCES,
  ),
  setting(
    "ratings.sources-show",
    "providers",
    { type: "checklist", options: RATING_FETCH_SOURCES, noun: "sources" },
    DEFAULT_RATING_FETCH_SOURCES,
  ),
  setting("ratings.fetch-after-scrape-movie", "providers", { type: "toggle" }, true),
  setting("ratings.fetch-after-scrape-show", "providers", { type: "toggle" }, true),
  setting("providers.prefer-original-title", "providers", { type: "toggle" }, false),
  setting(
    "metadata.nfo-format",
    "naming",
    {
      type: "choice",
      options: [
        { value: "kodi", label: "Kodi" },
        { value: "jellyfin", label: "Jellyfin" },
        { value: "emby", label: "Emby" },
        { value: "xbmc", label: "XBMC (Kodi before 17)" },
        { value: "plex", label: "Plex (Kodi format)" },
        { value: "mediaportal", label: "MediaPortal" },
      ],
    },
    "kodi",
  ),
  setting(
    "metadata.nfo-mediaportal-style",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-mediaportal-style", [
        "legacy",
        "moving-pictures",
        "myvideo",
      ]),
    },
    "legacy",
  ),
  setting("metadata.nfo-language", "naming", { type: "choice", options: LANGUAGES }, "en"),
  setting(
    "metadata.nfo-certification",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-certification", [
        "short",
        "short-mpaa",
        "medium",
        "medium-full",
        "large",
        "large-full",
        "technical",
        "technical2",
      ]),
    },
    "short",
  ),
  setting("metadata.nfo-create-outline", "naming", { type: "toggle" }, false),
  setting("metadata.nfo-outline-first-sentence", "naming", { type: "toggle" }, true),
  setting("metadata.nfo-single-studio", "naming", { type: "toggle" }, false),
  setting("metadata.nfo-date-added", "naming", { type: "toggle" }, true),
  setting(
    "metadata.nfo-date-added-source",
    "naming",
    {
      type: "choice",
      options: translatedOptions("metadata.nfo-date-added-source", [
        "added",
        "file-created",
        "file-modified",
        "release",
      ]),
    },
    "added",
  ),
  setting("metadata.nfo-artwork-urls", "naming", { type: "toggle" }, true),
  setting("metadata.nfo-trailer", "naming", { type: "toggle" }, true),
  setting("metadata.nfo-fileinfo", "naming", { type: "toggle" }, true),
  setting("metadata.nfo-external-audio", "naming", { type: "toggle" }, false),
  setting("metadata.nfo-lockdata", "naming", { type: "toggle" }, false),
  setting("metadata.nfo-clean", "naming", { type: "toggle" }, false),
  setting("renamer.keep-subfolders", "naming", { type: "toggle" }, true),
  setting(
    "files.deleted-mode",
    "library",
    {
      type: "choice",
      options: translatedOptions("files.deleted-mode", ["bin", "folder", "custom", "erase"]),
    },
    "bin",
  ),
  setting("files.trash-folder", "library", { type: "text", mono: true }, ""),
  setting("delete.confirm", "library", { type: "toggle" }, true),
  setting("server.permanent-delete", "advanced", { type: "toggle" }, false),
  setting(
    "files.auto-empty-days",
    "library",
    {
      type: "choice",
      options: translatedOptions("files.auto-empty-days", ["off", "7", "30", "90"]),
    },
    "off",
  ),
  setting("renamer.cleanup-unwanted", "library", { type: "toggle" }, false),
  setting(
    "renamer.unwanted-patterns",
    "library",
    { type: "text", placeholder: ".txt, .url, .html, .sfv", mono: true },
    ".txt, .url, .html, .sfv",
  ),
  setting("artwork.auto-select", "extras", { type: "toggle" }, true),
  setting(
    "metadata.artwork-names",
    "extras",
    {
      type: "choice",
      options: [
        { value: "prefixed", label: "<name>-poster" },
        { value: "short", label: "poster" },
        { value: "emby", label: "Emby (poster, fanart)" },
        { value: "jellyfin", label: "Jellyfin (poster, backdrop)" },
        { value: "plex", label: "Plex (poster, fanart)" },
      ],
    },
    "prefixed",
  ),
  setting(
    "artwork.poster-size",
    "extras",
    { type: "choice", options: translatedOptions("artwork.size", ARTWORK_SIZES) },
    "large",
  ),
  setting(
    "artwork.fanart-size",
    "extras",
    { type: "choice", options: translatedOptions("artwork.size", ARTWORK_SIZES) },
    "large",
  ),
  setting("artwork.other-resolutions", "extras", { type: "toggle" }, true),
  setting("artwork.languages", "extras", { type: "text", placeholder: "en, none", mono: true }, ""),
  setting("artwork.fanart-without-text", "extras", { type: "toggle" }, true),
  setting("artwork.any-language", "extras", { type: "toggle" }, true),
  setting(
    "artwork.collection-locations",
    "extras",
    {
      type: "choice",
      options: translatedOptions("artwork.collection-locations", [
        "movie-folder",
        "kodi",
        "automator",
        "both",
      ]),
    },
    "movie-folder",
  ),
  setting("artwork.extra-fanart-enabled", "extras", { type: "toggle" }, false),
  setting("artwork.extra-fanart", "extras", { type: "number", min: 0, max: 20, step: 1 }, 5),
  setting("artwork.extra-thumbs", "extras", { type: "toggle" }, false),
  setting("artwork.extra-thumbs-count", "extras", { type: "number", min: 0, max: 20, step: 1 }, 5),
  setting("artwork.extra-thumbs-resize", "extras", { type: "toggle" }, false),
  setting(
    "artwork.extra-thumbs-width",
    "extras",
    { type: "number", min: 100, max: 1920, step: 10, unit: "px" },
    480,
  ),
  setting("artwork.actor-images", "extras", { type: "toggle" }, false),
  setting(
    "player.kind",
    "library",
    {
      type: "choice",
      options: translatedOptions("player.kind", [
        "system",
        "mpv",
        "vlc",
        "mpc-hc",
        "iina",
        "custom",
      ]),
    },
    "system",
  ),
  setting(
    "player.path",
    "library",
    { type: "text", mono: true, placeholder: "C:\\Program Files\\mpv\\mpv.exe" },
    "",
  ),
  setting(
    "player.custom-command",
    "library",
    { type: "text", mono: true, placeholder: 'player.exe --fullscreen "{files}"' },
    "",
  ),
  setting(
    "advanced.size-units",
    "advanced",
    { type: "choice", options: translatedOptions("advanced.size-units", ["base10", "base2"]) },
    defaultSizeBase(),
  ),
  setting(
    "advanced.log-level",
    "advanced",
    {
      type: "choice",
      options: translatedOptions("advanced.log-level", ["error", "info", "debug"]),
    },
    "info",
  ),
  setting("torrents.source.torrentio", "torrents", { type: "toggle" }, true),
  setting("torrents.source.yts", "torrents", { type: "toggle" }, true),
  setting("torrents.source.eztv", "torrents", { type: "toggle" }, true),
  setting("torrents.source.apibay", "torrents", { type: "toggle" }, true),
  setting("torrents.source.knaben", "torrents", { type: "toggle" }, true),
  setting("torrents.source.nyaa", "torrents", { type: "toggle" }, true),
  setting("torrents.indexer-service.enabled", "torrents", { type: "toggle" }, true),
  setting(
    "torrents.torznab.url",
    "torrents",
    {
      type: "text",
      mono: true,
      placeholder: "http://localhost:9117/api/v2.0/indexers/all/results/torznab/api",
      format: "url",
    },
    "",
  ),
  setting(
    "torrents.indexer-service.solver-url",
    "torrents",
    { type: "text", mono: true, placeholder: "http://localhost:8191", format: "url" },
    "",
  ),
  setting(
    "torrents.url.torrentio",
    "torrents",
    { type: "text", mono: true, format: "url" },
    "https://torrentio.strem.fun",
  ),
  setting(
    "torrents.url.yts",
    "torrents",
    { type: "text", mono: true, format: "url" },
    "https://movies-api.accel.li",
  ),
  setting(
    "torrents.url.eztv",
    "torrents",
    { type: "text", mono: true, format: "url" },
    "https://eztvx.to",
  ),
  setting(
    "torrents.url.apibay",
    "torrents",
    { type: "text", mono: true, format: "url" },
    "https://apibay.org",
  ),
  setting(
    "torrents.url.knaben",
    "torrents",
    { type: "text", mono: true, format: "url" },
    "https://api.knaben.org",
  ),
  setting(
    "torrents.url.nyaa",
    "torrents",
    { type: "text", mono: true, format: "url" },
    "https://nyaa.si",
  ),
];

/** The settings the page, the search and the command palette offer. */
export const SETTINGS: readonly SettingDef[] = ALL_SETTINGS;

const BY_ID: ReadonlyMap<string, SettingDef> = new Map(ALL_SETTINGS.map((def) => [def.id, def]));

export function settingDef(id: string): SettingDef | undefined {
  return BY_ID.get(id);
}

/** Whether `value` is what the setting has when nothing is stored. */
export function isDefaultValue(def: SettingDef, value: unknown): boolean {
  return value === undefined || value === def.default;
}

/** Whether `value` is a legal value for the setting's control. */
export function isValidValue(def: SettingDef, value: unknown): value is SettingValue {
  const control = def.control;
  switch (control.type) {
    case "toggle":
      return typeof value === "boolean";
    case "choice":
      return typeof value === "string" && control.options.some((option) => option.value === value);
    case "number":
      return (
        typeof value === "number" &&
        Number.isInteger(value) &&
        value >= control.min &&
        value <= control.max
      );
    case "text":
      // A setting whose default is empty (a folder that is not set) accepts the empty value.
      return (
        typeof value === "string" &&
        value.length <= 400 &&
        (value.trim() !== "" || value === def.default || control.allowEmpty === true)
      );
    case "checklist": {
      if (typeof value !== "string") return false;
      const known = new Set(control.options.map((option) => option.value));
      const entries = value === "" ? [] : value.split(",");
      return entries.every((entry) => known.has(entry)) && new Set(entries).size === entries.length;
    }
  }
}
