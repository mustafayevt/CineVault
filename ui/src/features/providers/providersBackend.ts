import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  CredentialTestResult,
  KeyAttentionDto,
  ProviderAttributionDto,
  ProviderCatalogueDto,
  ProviderCommandError,
  ProviderCredentialsDto,
} from "../../bindings";
import { t } from "../../i18n/catalog";
import { createSyntheticProvidersBackend } from "./syntheticProviders";

export type ProviderCredentials = ProviderCredentialsDto;
export type CredentialField = ProviderCredentialsDto["fields"][number];
export type Importance = ProviderCredentialsDto["importance"];
export type CredentialState = ProviderCredentialsDto["state"];
export type TestResult = CredentialTestResult;
export type ProviderAttribution = ProviderAttributionDto;
export type KeyAttention = KeyAttentionDto;
export type ProviderCatalogue = ProviderCatalogueDto;
export type MetadataProviderInfo = ProviderCatalogueDto["providers"][number];
export type UnsupportedAddon = ProviderCatalogueDto["unsupportedAddons"][number];

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class ProviderRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ProviderRequestError";
    this.code = code;
  }
}

/**
 * Where provider credentials live. In the desktop app that is the Rust credential resolver: it
 * reports each provider's importance and state (Active, Missing or Rejected) and which fields
 * have a value, and never returns a key, so there is nothing secret to read back here.
 */
export interface ProvidersBackend {
  /** False in a plain browser, where there is no credential store. */
  readonly available: boolean;
  list: () => Promise<ProviderCredentials[]>;
  /** Saves the user's own value for a field. Returns the updated list. */
  set: (provider: string, field: string, value: string) => Promise<ProviderCredentials[]>;
  /** Removes everything the user saved for a provider. Returns the updated list. */
  remove: (provider: string) => Promise<ProviderCredentials[]>;
  /** Puts back what the last `remove` of a provider took away (its Undo). */
  restore: (provider: string) => Promise<ProviderCredentials[]>;
  /** Asks the provider whether it accepts the typed values, or the saved ones when none. */
  test: (provider: string, drafts: Record<string, string>) => Promise<TestResult>;
  /** The providers whose key is missing or rejected, for the banner. */
  attention: () => Promise<KeyAttention>;
  /** The credit each provider's terms require, with whether the provider is in use. */
  attributions: () => Promise<ProviderAttribution[]>;
  /**
   * Every metadata provider with whether it is ready, the Kodi add-ons that cannot be used and
   * the fields the universal provider combines. The choices themselves are `providers.*` settings.
   */
  catalogue: () => Promise<ProviderCatalogue>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: ProviderCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ProviderRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: ProvidersBackend = {
  available: true,
  list: async () => unwrap(await commands.listProviderCredentials()),
  set: async (provider, field, value) =>
    unwrap(await commands.setProviderCredential(provider, field, value)),
  remove: async (provider) => unwrap(await commands.removeProviderCredentials(provider)),
  restore: async (provider) => unwrap(await commands.restoreProviderCredentials(provider)),
  test: async (provider, drafts) =>
    unwrap(
      await commands.testProviderCredentials(
        provider,
        Object.entries(drafts).map(([field, value]) => ({ field, value })),
      ),
    ),
  attention: async () => unwrap(await commands.getKeyAttention()),
  attributions: async () => unwrap(await commands.listProviderAttributions()),
  catalogue: async () => unwrap(await commands.listMetadataProviders()),
};

function desktopOnly(): Promise<never> {
  return Promise.reject(new ProviderRequestError("unavailable", t("providers.desktopOnly")));
}

/** The backend of a plain browser (development): nothing is listed and changes are refused. */
export const unavailableBackend: ProvidersBackend = {
  available: false,
  list: () => Promise.resolve([]),
  set: desktopOnly,
  remove: desktopOnly,
  restore: desktopOnly,
  test: desktopOnly,
  attention: () => Promise.resolve({ required: [], recommended: [], identification_paused: false }),
  attributions: () => Promise.resolve([]),
  catalogue: () => Promise.resolve({ providers: [], unsupportedAddons: [], universalFields: [] }),
};

let override: ProvidersBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setProvidersBackend(backend: ProvidersBackend | null): void {
  override = backend;
}

let synthetic: ProvidersBackend | null = null;

export function currentProvidersBackend(): ProvidersBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriBackend;
  // `?synthetic-keys` (or `?synthetic-keys=mixed`) develops the page in a plain browser against
  // an in-memory list; without it a browser has no credential store.
  const params = new URLSearchParams(window.location.search);
  if (params.has("synthetic-keys") || params.has("first-run")) {
    synthetic ??= createSyntheticProvidersBackend(params.get("synthetic-keys") === "mixed");
    return synthetic;
  }
  return unavailableBackend;
}

/**
 * The banner's content computed from a provider list, for the Providers page, which already
 * holds the list. Matches the backend: providers that are not active, required and recommended
 * only.
 */
export function attentionOf(providers: readonly ProviderCredentials[]): KeyAttention {
  const pick = (importance: Importance) =>
    providers
      .filter((provider) => provider.importance === importance && provider.state !== "active")
      .map((provider) => ({ id: provider.id, name: provider.name, state: provider.state }));
  const required = pick("required");
  return {
    required,
    recommended: pick("recommended"),
    identification_paused: required.length > 0,
  };
}
