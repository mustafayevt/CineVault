import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createSyntheticDetailBackend } from "./syntheticDetail";
import { setDetailBackend } from "./detailBackend";
import type { DetailCredit } from "./detailBackend";
import { useCastPortraits } from "./useCastPortraits";

function wrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
}

const image = { key: "0".repeat(31) + "1", thumbhash: null, ambient: null };
const onDisk = { key: "0".repeat(31) + "2", thumbhash: null, ambient: null };

function credit(name: string, over: Partial<DetailCredit> = {}): DetailCredit {
  return {
    name,
    role: "",
    kind: "actor",
    portrait: null,
    imdb_id: null,
    tmdb_id: null,
    ...over,
  };
}

afterEach(() => setDetailBackend(null));

describe("cast portraits (OP-1)", () => {
  it("fills the people without a portrait file and leaves the ones that have one", async () => {
    const castPortraits = vi.fn(() =>
      Promise.resolve([
        { name: "Zendaya", portrait: image },
        { name: "Timothée Chalamet", portrait: image },
      ]),
    );
    setDetailBackend({ ...createSyntheticDetailBackend(), castPortraits });
    const credits = [
      credit("Zendaya"),
      credit("Timothée Chalamet", { portrait: onDisk }),
      credit("Denis Villeneuve", { kind: "director" }),
    ];
    const { result } = renderHook(() => useCastPortraits("7", credits), { wrapper: wrapper() });
    await waitFor(() => expect(result.current[0]?.portrait).toEqual(image));
    // The file in `.actors` comes first; crew have no portrait.
    expect(result.current[1]?.portrait).toEqual(onDisk);
    expect(result.current[2]?.portrait).toBeNull();
    expect(castPortraits).toHaveBeenCalledWith("7");
  });

  it("asks for nothing when every actor already has a portrait", () => {
    const castPortraits = vi.fn(() => Promise.resolve([]));
    setDetailBackend({ ...createSyntheticDetailBackend(), castPortraits });
    renderHook(() => useCastPortraits("7", [credit("Zendaya", { portrait: onDisk })]), {
      wrapper: wrapper(),
    });
    expect(castPortraits).not.toHaveBeenCalled();
  });
});

describe("crew listed once per person (QA-34)", () => {
  it("joins the parts of a person who appears twice", async () => {
    const { mergeCrew } = await import("./castModel");
    const crew = [
      credit("Vince Gilligan", { kind: "director", role: "" }),
      credit("Vince Gilligan", { kind: "writer", role: "" }),
      credit("Peter Gould", { kind: "writer", role: "Writer" }),
    ];
    const merged = mergeCrew(crew, (person) => (person.role === "" ? person.kind : person.role));
    expect(merged.map((person) => [person.name, person.role])).toEqual([
      ["Vince Gilligan", "director, writer"],
      ["Peter Gould", "Writer"],
    ]);
  });
});
