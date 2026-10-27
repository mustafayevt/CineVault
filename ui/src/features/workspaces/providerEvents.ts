import type { ProviderEvent } from "../../bindings";

/**
 * What a provider search sends over its Channel: Started, Results, Failed{reason,
 * retryable}, NeedsKey and Done. It is the type the backend generates (`ProviderEvent<T>` in
 * `bindings.ts`); the UI reads it through here so a workspace does not care whether the events
 * come from the desktop shell or from the synthetic backend.
 */
export type { ProviderEvent };

/** The `reason` of a failed event that means the provider rejected the saved key. */
export const KEY_REJECTED = "key-rejected";

/**
 * The `reason` of a failed event that means the provider cannot be asked right now, because its
 * site asks for a human check: shown as a quiet "unavailable right now", never as a failure.
 */
export const UNAVAILABLE = "unavailable";

/** What a search asks the backend for. */
export interface ProviderSearchRequest {
  searchId: string;
  query: string;
  /** Extra parameters of the flow (the artwork type, the language, ...). */
  params?: Readonly<Record<string, string | number | boolean>>;
}

/**
 * The calls a workspace makes: start a search that streams events for `searchId`, ask one
 * provider again, and cancel a search. `cancel_search` and `retry_provider` are the commands of
 * the same names.
 */
export interface ProviderSearchTransport<T> {
  start: (request: ProviderSearchRequest, onEvent: (event: ProviderEvent<T>) => void) => void;
  retry: (searchId: string, provider: string) => void;
  cancel: (searchId: string) => void;
}

/** How one provider is doing in a search. */
export type ProviderState =
  "searching" | "done" | "failed" | "rejected" | "needs-key" | "unavailable";

export interface ProviderStatus {
  provider: string;
  state: ProviderState;
  /** Results this provider has delivered so far. */
  count: number;
  /** The reason a failed provider gave, in the backend's words. */
  reason?: string;
  /** Whether Retry can help (a timeout can be retried, a malformed reply may not be). */
  retryable: boolean;
}

/** A result with the provider that returned it. */
export interface ProviderItem<T> {
  provider: string;
  item: T;
}
