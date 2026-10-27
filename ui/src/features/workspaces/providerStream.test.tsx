import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { createFakeProviderTransport } from "./fakeProviderTransport";
import type { FakeProviderScript } from "./fakeProviderTransport";
import type { ProviderStatus } from "./providerEvents";
import { applyProviderEvent, useProviderStream } from "./useProviderStream";

function Harness({
  query = "heat",
  transport,
}: {
  query?: string;
  transport: ReturnType<typeof createFakeProviderTransport<string>>;
}) {
  const stream = useProviderStream<string>({ transport: transport.transport, query });
  return (
    <div>
      <ul aria-label="results">
        {stream.items.map(({ provider, item }) => (
          <li key={`${provider}-${item}`}>{`${provider}:${item}`}</li>
        ))}
      </ul>
      <p>{stream.searching ? "searching" : "finished"}</p>
    </div>
  );
}

function setup(script: Readonly<Record<string, FakeProviderScript<string>>>) {
  return createFakeProviderTransport<string>(script);
}

describe("useProviderStream", () => {
  it("cancels the search on unmount and when the query changes", async () => {
    const fake = setup({ tmdb: { delayMs: 50, items: ["a"] } });
    const { rerender, unmount } = render(<Harness transport={fake} query="heat" />);
    expect(fake.calls.started).toHaveLength(1);
    const first = fake.calls.started[0]?.searchId;

    rerender(<Harness transport={fake} query="heat 1995" />);
    expect(fake.calls.cancelled).toEqual([first]);
    expect(fake.calls.started).toHaveLength(2);
    const second = fake.calls.started[1]?.searchId;
    expect(second).not.toBe(first);

    unmount();
    expect(fake.calls.cancelled).toEqual([first, second]);
  });

  it("ignores events of a search that was replaced", async () => {
    const fake = setup({ tmdb: { delayMs: 60, items: ["late"] } });
    const { rerender } = render(<Harness transport={fake} query="one" />);
    rerender(<Harness transport={fake} query="two" />);
    // A stray event of the first search is dropped; the second search's own results arrive.
    await waitFor(() => expect(screen.getByText("tmdb:late")).toBeInTheDocument());
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
  });

  it("accepts hand-made events", async () => {
    const fake = setup({});
    render(<Harness transport={fake} />);
    act(() => fake.emit({ kind: "started", provider: "tmdb" }));
    act(() => fake.emit({ kind: "results", provider: "tmdb", items: ["m"] }));
    expect(screen.getByText("tmdb:m")).toBeInTheDocument();
  });
});

describe("applyProviderEvent", () => {
  it("keeps the order providers first spoke in and marks the rest done on Done", () => {
    let state: Parameters<typeof applyProviderEvent<never>>[0] = {
      items: [],
      providers: [] as ProviderStatus[],
      done: false,
    };
    state = applyProviderEvent<never>(state, { kind: "started", provider: "b" });
    state = applyProviderEvent<never>(state, { kind: "started", provider: "a" });
    state = applyProviderEvent<never>(state, { kind: "done" });
    expect(state.providers.map((p) => [p.provider, p.state])).toEqual([
      ["b", "done"],
      ["a", "done"],
    ]);
    expect(state.done).toBe(true);
  });
});

describe("applyProviderEvent: a provider that answers in batches", () => {
  const start = { items: [], providers: [], done: false };

  it("keeps the provider searching and counts what it has handed over so far", () => {
    const first = applyProviderEvent<string>(start, {
      kind: "partial",
      provider: "tmdb",
      items: ["a", "b"],
    });
    expect(first.items.map((entry) => entry.item)).toEqual(["a", "b"]);
    expect(first.providers).toEqual([
      { provider: "tmdb", state: "searching", count: 2, retryable: false },
    ]);
    const last = applyProviderEvent<string>(first, {
      kind: "results",
      provider: "tmdb",
      items: ["c"],
    });
    expect(last.items.map((entry) => entry.item)).toEqual(["a", "b", "c"]);
    expect(last.providers[0]).toMatchObject({ state: "done", count: 3 });
  });

  it("takes an update as more of the same items, without changing the count or the state", () => {
    const first = applyProviderEvent<string>(start, {
      kind: "partial",
      provider: "tmdb",
      items: ["a"],
    });
    const updated = applyProviderEvent<string>(first, {
      kind: "update",
      provider: "tmdb",
      items: ["a2"],
    });
    expect(updated.items.map((entry) => entry.item)).toEqual(["a", "a2"]);
    expect(updated.providers[0]).toMatchObject({ state: "searching", count: 1 });
  });

  it("shows a batch while another provider is still working and never waits for the slowest", async () => {
    const fake = createFakeProviderTransport<string>({
      tmdb: { delayMs: 10, items: ["a", "b", "c"], batchSize: 1, batchMs: 200 },
      fanart: { delayMs: 5000, items: ["z"] },
    });
    render(<Harness transport={fake} />);
    expect(await screen.findByText("tmdb:a")).toBeInTheDocument();
    expect(screen.getByText("searching", { selector: "p" })).toBeInTheDocument();
    expect(await screen.findByText("tmdb:c")).toBeInTheDocument();
    expect(screen.queryByText("fanart:z")).toBeNull();
  });
});
