import type { ProviderStatus } from "./providerEvents";

/**
 * Whether a finished search heard from no provider at all: every one failed, was rejected or
 * needs a key. An empty list then says that nobody answered, not that nothing was found
 * (QA final-24).
 */
export function noProviderAnswered(providers: readonly ProviderStatus[]): boolean {
  return (
    providers.length > 0 &&
    providers.every((status) => status.state !== "searching" && status.state !== "done")
  );
}
