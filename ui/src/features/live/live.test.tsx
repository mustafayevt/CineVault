import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { ItemsChanged } from "../../bindings";
import { useHighlightStore } from "./highlightStore";
import { itemPartKey, listKeys } from "./itemQueries";
import { COLLECT_MS, setItemsChangedSource, useItemsChangedSync } from "./useItemsChanged";

/** A source the test pushes changes into by hand. */
function fakeSource() {
  let handler: ((change: ItemsChanged) => void) | null = null;
  const stopped = { count: 0 };
  setItemsChangedSource((next) => {
    handler = next;
    return Promise.resolve(() => {
      stopped.count += 1;
    });
  });
  return {
    stopped,
    emit: (change: ItemsChanged) => {
      if (handler === null) throw new Error("nobody listens yet");
      handler(change);
    },
  };
}

const fetches = { ratings: 0, files: 0, artworkOfOther: 0, counts: 0, organize: 0 };

function Probe({ id = "5" }: { id?: string }) {
  useItemsChangedSync();
  useQuery({
    queryKey: itemPartKey(id, "ratings"),
    queryFn: () => ++fetches.ratings,
    staleTime: Infinity,
  });
  useQuery({
    queryKey: itemPartKey(id, "files"),
    queryFn: () => ++fetches.files,
    staleTime: Infinity,
  });
  useQuery({
    queryKey: itemPartKey("9", "artwork"),
    queryFn: () => ++fetches.artworkOfOther,
    staleTime: Infinity,
  });
  useQuery({
    queryKey: ["organize", "stamp"],
    queryFn: () => ++fetches.organize,
    staleTime: Infinity,
  });
  useQuery({
    queryKey: ["lifecycle", "counts", "movies"],
    queryFn: () => ++fetches.counts,
    staleTime: Infinity,
  });
  return <div />;
}

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <Probe />
    </QueryClientProvider>,
  );
  return client;
}

async function settle(ms: number) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

beforeEach(() => {
  Object.assign(fetches, { ratings: 0, files: 0, artworkOfOther: 0, counts: 0, organize: 0 });
  useHighlightStore.getState().clear();
});

afterEach(() => {
  setItemsChangedSource(null);
  useHighlightStore.getState().clear();
});

describe("live updates from the items-changed event", () => {
  it("refetches the ratings of the title that changed and nothing else", async () => {
    const source = fakeSource();
    mount();
    await waitFor(() => expect(fetches.ratings).toBe(1));
    expect(fetches.files).toBe(1);

    act(() => source.emit({ items: ["5"], parts: ["ratings"] }));
    await settle(COLLECT_MS + 60);

    await waitFor(() => expect(fetches.ratings).toBe(2));
    // The files of the same title, the artwork of another title and the counts stay as they were.
    expect(fetches.files).toBe(1);
    expect(fetches.artworkOfOther).toBe(1);
    expect(fetches.counts).toBe(1);
  });

  it("does not touch a title that was not in the event", async () => {
    const source = fakeSource();
    mount();
    await waitFor(() => expect(fetches.artworkOfOther).toBe(1));

    act(() => source.emit({ items: ["5"], parts: ["artwork"] }));
    await settle(COLLECT_MS + 60);

    expect(fetches.artworkOfOther).toBe(1);
    expect(fetches.ratings).toBe(1);
  });

  it("applies a burst of events once and refreshes the pipeline counts after a stage change", async () => {
    const source = fakeSource();
    mount();
    await waitFor(() => expect(fetches.counts).toBe(1));

    act(() => {
      source.emit({ items: ["5"], parts: ["ratings"] });
      source.emit({ items: ["5"], parts: ["ratings", "stage"] });
      source.emit({ items: ["5"], parts: ["ratings"] });
    });
    await settle(COLLECT_MS + 80);

    await waitFor(() => expect(fetches.ratings).toBe(2));
    await waitFor(() => expect(fetches.counts).toBe(2));
  });

  it("asks the Organize page to plan again when the files or the stage of titles change", async () => {
    const source = fakeSource();
    mount();
    await waitFor(() => expect(fetches.organize).toBe(1));

    act(() => source.emit({ items: ["5"], parts: ["ratings"] }));
    await settle(COLLECT_MS + 60);
    expect(fetches.organize).toBe(1);

    act(() => source.emit({ items: ["5"], parts: ["files"] }));
    await settle(COLLECT_MS + 80);
    await waitFor(() => expect(fetches.organize).toBe(2));
  });

  it("stops listening when the shell goes away", async () => {
    const source = fakeSource();
    const view = render(
      <QueryClientProvider client={new QueryClient()}>
        <Probe />
      </QueryClientProvider>,
    );
    await settle(10);
    view.unmount();
    await waitFor(() => expect(source.stopped.count).toBe(1));
  });
});

// qa2-02, qa2-07: a change to a title's files refreshes Home's size and the missing review live.
describe("the lists a change refreshes", () => {
  it("include Home and the missing review when files, details or the stage change", () => {
    for (const part of ["files", "details", "stage"] as const) {
      const keys = listKeys(new Set([part])).map((key) => JSON.stringify(key));
      expect(keys).toContain(JSON.stringify(["home"]));
      expect(keys).toContain(JSON.stringify(["library-ops", "review"]));
    }
    expect(listKeys(new Set(["ratings"])).map((key) => JSON.stringify(key))).not.toContain(
      JSON.stringify(["home"]),
    );
  });
});
