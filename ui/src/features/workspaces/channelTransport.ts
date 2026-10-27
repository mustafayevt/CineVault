import { Channel } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  ProviderEvent,
  ProviderSearchRequest,
  ProviderSearchTransport,
} from "./providerEvents";

/** The part of a Tauri Channel the transport uses; tests pass a plain object. */
export interface ChannelLike<T> {
  onmessage: (event: T) => void;
}

/** Starts one search: the flow's own streaming command, with the channel its events go down. */
export type StartSearch<T> = (
  request: ProviderSearchRequest,
  channel: ChannelLike<ProviderEvent<T>>,
) => Promise<unknown>;

/**
 * The real transport of a streaming flow: `start` calls the flow's command (for example
 * `stream_artwork_candidates`) with a Channel, and the events arrive on it; `retry` and
 * `cancel` are the `retry_provider` and `cancel_search` commands every flow shares. A command
 * that cannot start the search ends the stream with one failure instead of leaving it waiting,
 * and a retry the backend refuses (the search is over, the provider is still answering) puts
 * the provider back to failed with the reason, so its chip never stays on "searching".
 */
export function channelTransport<T>(
  start: StartSearch<T>,
  makeChannel: () => ChannelLike<ProviderEvent<T>> = () => new Channel<ProviderEvent<T>>(),
): ProviderSearchTransport<T> {
  // Where the events of each running search go, so a refused retry can answer on it.
  const listeners = new Map<string, (event: ProviderEvent<T>) => void>();
  return {
    start: (request, onEvent) => {
      const channel = makeChannel();
      channel.onmessage = onEvent;
      listeners.set(request.searchId, onEvent);
      start(request, channel).catch((error: unknown) => {
        onEvent({
          kind: "failed",
          provider: "search",
          reason: error instanceof Error ? error.message : "The search could not start.",
          retryable: false,
        });
        onEvent({ kind: "done" });
      });
    },
    retry: (searchId, provider) => {
      const refused = (reason: string, retryable: boolean) => {
        const onEvent = listeners.get(searchId);
        if (onEvent === undefined) return;
        onEvent({ kind: "failed", provider, reason, retryable });
        onEvent({ kind: "done" });
      };
      commands
        .retryProvider(searchId, provider)
        .then((result) => {
          if (result.status === "error") {
            refused(result.error.message, result.error.code === "still_running");
          }
        })
        .catch((error: unknown) => {
          refused(
            error instanceof Error ? error.message : "The provider could not be asked again.",
            true,
          );
        });
    },
    cancel: (searchId) => {
      listeners.delete(searchId);
      void commands.cancelSearch(searchId);
    },
  };
}
