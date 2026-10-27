import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it } from "vitest";

import { setDrivesBackend } from "../drives/drivesBackend";
import { NO_DRIVES } from "../drives/drivesModel";
import type { Drive } from "../drives/drivesModel";
import { useSidebarDrives } from "./useSidebarDrives";

afterEach(() => {
  setDrivesBackend(null);
});

const drive = (over: Partial<Drive>): Drive => ({
  id: "D:",
  name: "D: Media",
  kind: "hdd",
  totalBytes: 7.28e12,
  freeBytes: 1.21e12,
  folders: [],
  notInLibraryBytes: null,
  quality: { p2160: 0, p1080: 0, p720: 0, sd: 0 },
  online: true,
  lastSeenMs: null,
  low: false,
  trashBytes: 0,
  binBytes: 0,
  customTrash: null,
  ...over,
});

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("the drives of the sidebar", () => {
  it("are the drives of the drives overview, with their low and offline state", async () => {
    setDrivesBackend({
      overview: () =>
        Promise.resolve({
          updatedMs: 1,
          drives: [
            drive({ id: "C:", name: "C: System", totalBytes: 931e9, freeBytes: 41e9, low: true }),
            drive({ id: "Z:", name: "Z: NAS", freeBytes: 0, online: false }),
          ],
        }),
      onChanged: () => () => undefined,
      trashContents: () => Promise.resolve({ entries: [], bytes: 0 }),
      emptyTrash: () => Promise.resolve({ bytes: 0, problems: [] }),
    });
    const { result } = renderHook(() => useSidebarDrives(), { wrapper });
    await waitFor(() => expect(result.current).toHaveLength(2));
    expect(result.current[0]).toEqual({
      id: "C:",
      name: "C: System",
      totalBytes: 931e9,
      freeBytes: 41e9,
      online: true,
      low: true,
    });
    expect(result.current[1]).toMatchObject({ id: "Z:", online: false, low: false });
  });

  it("are none while the library has no folder on a drive", async () => {
    setDrivesBackend({
      overview: () => Promise.resolve(NO_DRIVES),
      onChanged: () => () => undefined,
      trashContents: () => Promise.resolve({ entries: [], bytes: 0 }),
      emptyTrash: () => Promise.resolve({ bytes: 0, problems: [] }),
    });
    const { result } = renderHook(() => useSidebarDrives(), { wrapper });
    await waitFor(() => expect(result.current).toEqual([]));
  });
});
