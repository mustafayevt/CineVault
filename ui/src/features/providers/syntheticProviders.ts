import { t } from "../../i18n/catalog";
import { ProviderRequestError } from "./providersBackend";
import { attentionOf } from "./providersBackend";
import type {
  CredentialState,
  Importance,
  ProviderCredentials,
  ProvidersBackend,
} from "./providersBackend";

interface Spec {
  id: string;
  name: string;
  importance: Importance;
  fields: { key: string; secret: boolean; required: boolean }[];
}

const KEY = [{ key: "api_key", secret: true, required: true }];

/**
 * The providers as the desktop app lists them (in catalogue rank order), for developing the
 * page in a plain browser.
 */
const SPECS: readonly Spec[] = [
  { id: "tmdb", name: "TMDB", importance: "required", fields: KEY },
  { id: "omdb", name: "OMDb", importance: "recommended", fields: KEY },
  {
    id: "tvdb",
    name: "TheTVDB",
    importance: "recommended",
    fields: [...KEY, { key: "pin", secret: true, required: false }],
  },
  { id: "fanart", name: "Fanart.tv", importance: "recommended", fields: KEY },
  { id: "moviemeter", name: "MovieMeter", importance: "optional", fields: KEY },
  { id: "mdblist", name: "MDBList", importance: "optional", fields: KEY },
  {
    id: "anidb",
    name: "AniDB",
    importance: "optional",
    fields: [
      { key: "api_key", secret: true, required: true },
      { key: "client_version", secret: false, required: false },
    ],
  },
  {
    id: "trakt",
    name: "Trakt",
    importance: "optional",
    fields: [
      { key: "client_id", secret: true, required: true },
      { key: "client_secret", secret: true, required: false },
    ],
  },
  {
    id: "mpdb",
    name: "MPDb.tv",
    importance: "optional",
    fields: [
      { key: "api_key", secret: true, required: true },
      { key: "username", secret: false, required: true },
      { key: "subscription_key", secret: true, required: true },
    ],
  },
  {
    id: "thesportsdb",
    name: "TheSportsDB",
    importance: "optional",
    fields: [{ key: "api_key", secret: true, required: false }],
  },
];

/** The providers that search and scrape titles. */
const METADATA = new Set(["tmdb", "tvdb", "omdb", "moviemeter", "anidb", "mpdb", "trakt"]);

/** The fields the universal provider combines. */
const UNIVERSAL_FIELDS = [
  "title",
  "original_title",
  "tagline",
  "plot",
  "year",
  "release_date",
  "rating",
  "runtime",
  "certification",
  "genres",
  "country",
  "studio",
  "collection",
  "actors",
  "crew",
  "trailer",
];

/**
 * An in-memory stand-in for the credential resolver. `mixed` starts with an Active and a Rejected
 * provider so every state can be seen; otherwise every provider is Missing. It keeps what is
 * typed only in this tab and accepts any key except `bad`.
 */
export function createSyntheticProvidersBackend(mixed = false): ProvidersBackend {
  const saved = new Set<string>(mixed ? ["omdb.api_key", "fanart.api_key", "tvdb.api_key"] : []);
  const rejected = new Set<string>(mixed ? ["fanart"] : []);
  const removed = new Map<string, string[]>();
  const list = (): ProviderCredentials[] =>
    SPECS.map((spec, index) => {
      const fields = spec.fields.map((field) => ({
        key: field.key,
        kind: field.secret ? ("secret" as const) : ("text" as const),
        required: field.required,
        present: saved.has(`${spec.id}.${field.key}`),
      }));
      const configured = fields.some((field) => field.present);
      const active = fields.filter((field) => field.required).every((field) => field.present);
      const state: CredentialState =
        configured && rejected.has(spec.id)
          ? "rejected"
          : active && configured
            ? "active"
            : "missing";
      return {
        id: spec.id,
        name: spec.name,
        registration_url: `https://example.test/${spec.id}`,
        importance: spec.importance,
        rank: index + 1,
        fields,
        active,
        configured,
        state,
        missing: fields.filter((field) => field.required && !field.present).map((f) => f.key),
        testable: true,
      };
    });
  return {
    available: true,
    list: () => Promise.resolve(list()),
    set: (provider, field, value) => {
      if (value.trim() === "") {
        return Promise.reject(new ProviderRequestError("invalid", t("providers.testFailed")));
      }
      saved.add(`${provider}.${field}`);
      rejected.delete(provider);
      return Promise.resolve(list());
    },
    remove: (provider) => {
      const taken = [...saved].filter((key) => key.startsWith(`${provider}.`));
      for (const key of taken) saved.delete(key);
      removed.set(provider, taken);
      rejected.delete(provider);
      return Promise.resolve(list());
    },
    restore: (provider) => {
      for (const key of removed.get(provider) ?? []) saved.add(key);
      removed.delete(provider);
      return Promise.resolve(list());
    },
    test: (_provider, drafts) =>
      Promise.resolve(
        Object.values(drafts).includes("bad")
          ? { ok: false, code: "rejected", message: "Invalid API key" }
          : { ok: true, code: null, message: null },
      ),
    attention: () => Promise.resolve(attentionOf(list())),
    attributions: () => Promise.resolve([]),
    catalogue: () =>
      Promise.resolve({
        providers: [
          ...SPECS.filter((spec) => METADATA.has(spec.id)).map((spec) => {
            const active = spec.fields
              .filter((field) => field.required)
              .every((field) => saved.has(`${spec.id}.${field.key}`));
            return {
              id: spec.id,
              name: spec.name,
              movies: true,
              shows: spec.id !== "mpdb",
              ready: active,
              kodi: false,
              canFallback: true,
            };
          }),
          {
            id: "universal",
            name: "Universal",
            movies: true,
            shows: true,
            ready: true,
            kodi: false,
            canFallback: false,
          },
        ],
        unsupportedAddons: [
          {
            addonId: "metadata.example.python",
            addonName: "Example Python scraper",
            content: "movies",
            reason: "Python scrapers cannot be run.",
          },
        ],
        universalFields: UNIVERSAL_FIELDS,
      }),
  };
}
