import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useToastStore } from "../toasts/toastStore";
import { setCurationBackend } from "./curationBackend";
import type { CurationBackend } from "./curationBackend";
import { useClearNewOnOpen } from "./newFlagActions";

function backend(flagged: readonly string[]): CurationBackend {
  let still = [...flagged];
  return {
    available: true,
    clearNew: vi.fn((ids: readonly string[]) => {
      const changed = ids.filter((id) => still.includes(id));
      still = still.filter((id) => !changed.includes(id));
      return Promise.resolve(changed);
    }),
    clearNewOnOpen: vi.fn((id: string) => {
      const changed = still.filter((flagged) => flagged === id);
      still = still.filter((flagged) => flagged !== id);
      return Promise.resolve(changed);
    }),
    clearAllNew: vi.fn(() => {
      const changed = still;
      still = [];
      // In the fixtures, ids starting with "e" are episodes of show "s".
      const titles = new Set(changed.map((id) => (id.startsWith("e") ? "s" : id))).size;
      return Promise.resolve({ changed, titles });
    }),
    restoreNew: vi.fn((ids: readonly string[]) => {
      still = [...still, ...ids];
      return Promise.resolve();
    }),
  };
}

function wrapper(client = new QueryClient()) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
});

afterEach(() => {
  setCurationBackend(null);
});

describe("opening a title", () => {
  it("clears its own NEW marker only and leaves the other titles alone", async () => {
    const fake = backend(["7", "8"]);
    setCurationBackend(fake);
    function Page() {
      useClearNewOnOpen("7");
      return null;
    }
    render(<Page />, { wrapper: wrapper() });
    // Not the selection's Clear NEW: that one also clears a show's episodes, which an opened
    // show keeps.
    await waitFor(() => expect(fake.clearNewOnOpen).toHaveBeenCalledWith("7"));
    expect(fake.clearNew).not.toHaveBeenCalled();
    expect(fake.clearAllNew).not.toHaveBeenCalled();
  });

  it("stays quiet when the backend cannot clear it", async () => {
    const fake = backend(["7"]);
    fake.clearNewOnOpen = vi.fn(() => Promise.reject(new Error("offline")));
    setCurationBackend(fake);
    function Page() {
      useClearNewOnOpen("7");
      return null;
    }
    render(<Page />, { wrapper: wrapper() });
    await waitFor(() => expect(fake.clearNewOnOpen).toHaveBeenCalled());
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });
});
