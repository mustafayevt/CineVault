import { beforeEach, describe, expect, it, vi } from "vitest";

import { channelTransport } from "./channelTransport";
import type { ChannelLike } from "./channelTransport";
import type { ProviderEvent } from "./providerEvents";

const calls = vi.hoisted(() => ({
  retry: [] as [string, string][],
  cancel: [] as string[],
  /** What the next retry answers; `null` is success. */
  refuse: null as { code: string; message: string } | null,
}));

vi.mock("../../bindings", () => ({
  commands: {
    retryProvider: (searchId: string, provider: string) => {
      calls.retry.push([searchId, provider]);
      return Promise.resolve(
        calls.refuse === null
          ? { status: "ok", data: null }
          : { status: "error", error: calls.refuse },
      );
    },
    cancelSearch: (searchId: string) => {
      calls.cancel.push(searchId);
      return Promise.resolve(true);
    },
  },
}));

function fakeChannel() {
  const channel: ChannelLike<ProviderEvent<number>> = { onmessage: () => undefined };
  return channel;
}

beforeEach(() => {
  calls.retry.length = 0;
  calls.cancel.length = 0;
  calls.refuse = null;
});

describe("the channel transport", () => {
  it("starts the flow's command with a channel and hands every event to the stream", async () => {
    const channel = fakeChannel();
    const received: ProviderEvent<number>[] = [];
    const started: string[] = [];
    const transport = channelTransport<number>(
      (request, given) => {
        started.push(request.searchId);
        expect(given).toBe(channel);
        return Promise.resolve();
      },
      () => channel,
    );

    transport.start({ searchId: "s1", query: "heat" }, (event) => received.push(event));
    channel.onmessage({ kind: "started", provider: "tmdb" });
    channel.onmessage({ kind: "results", provider: "tmdb", items: [1, 2] });
    channel.onmessage({ kind: "done" });

    expect(started).toEqual(["s1"]);
    expect(received.map((event) => event.kind)).toEqual(["started", "results", "done"]);
  });

  it("sends retry and cancel to the commands every flow shares", () => {
    const transport = channelTransport<number>(() => Promise.resolve(), fakeChannel);
    transport.retry("s1", "omdb");
    transport.cancel("s1");
    expect(calls.retry).toEqual([["s1", "omdb"]]);
    expect(calls.cancel).toEqual(["s1"]);
  });

  it("puts the provider back to failed when the backend refuses the retry", async () => {
    const received: ProviderEvent<number>[] = [];
    const transport = channelTransport<number>(() => Promise.resolve(), fakeChannel);
    transport.start({ searchId: "s3", query: "x" }, (event) => received.push(event));
    calls.refuse = { code: "unknown_search", message: "That search is over. Search again." };
    transport.retry("s3", "omdb");
    await vi.waitFor(() => expect(received).toHaveLength(2));
    expect(received[0]).toEqual({
      kind: "failed",
      provider: "omdb",
      reason: "That search is over. Search again.",
      retryable: false,
    });
    expect(received[1]).toEqual({ kind: "done" });

    // A retry that is accepted answers on the channel, not here.
    calls.refuse = null;
    transport.retry("s3", "omdb");
    await Promise.resolve();
    await Promise.resolve();
    expect(received).toHaveLength(2);
  });

  it("ends the stream with one failure when the command cannot start the search", async () => {
    const received: ProviderEvent<number>[] = [];
    const transport = channelTransport<number>(
      () => Promise.reject(new Error("The library could not be opened.")),
      fakeChannel,
    );
    transport.start({ searchId: "s2", query: "x" }, (event) => received.push(event));
    await vi.waitFor(() => expect(received).toHaveLength(2));
    expect(received[0]).toEqual({
      kind: "failed",
      provider: "search",
      reason: "The library could not be opened.",
      retryable: false,
    });
    expect(received[1]).toEqual({ kind: "done" });
  });
});
