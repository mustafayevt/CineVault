import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { JobProgress } from "../../bindings";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { useNotificationStore } from "../notifications/notificationStore";
import { EMPTY_SIDEBAR_DATA, useSidebarDataStore } from "../sidebar/sidebarData";
import { settingDef } from "../settings/schema";
import { useToastStore } from "../toasts/toastStore";
import { setToolsBackend } from "../tools/toolsBackend";
import { runRescan, useScanUiStore } from "./scanActions";
import { ScanRequestError, setScanBackend } from "./scanBackend";
import type { RescanStarted, ScanBackend, SourceHealth } from "./scanBackend";
import {
  SCAN_REFRESH_MS,
  SOURCE_HEALTH_KEY,
  useRefreshAfterJobs,
  useRefreshDuringScan,
  useSidebarSources,
} from "./useSourceHealth";

function health(overrides: Partial<SourceHealth> = {}): SourceHealth {
  return {
    id: 1,
    kind: "movie",
    path: "D:\\Media\\Movies",
    online: true,
    titles: 49,
    missing: 0,
    pending_media_info: 0,
    ...overrides,
  };
}

function fakeBackend(overrides: Partial<ScanBackend> = {}): ScanBackend {
  return {
    available: true,
    rescan: vi.fn(() => Promise.resolve({ job_id: 7, sources: 2, media_info: "ready" as const })),
    findMissing: vi.fn(() => Promise.resolve([])),
    sourceHealth: vi.fn(() => Promise.resolve([health()])),
    readPendingMediaInfo: vi.fn(() => Promise.resolve("ready" as const)),
    ...overrides,
  };
}

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
  useNotificationStore.setState({ items: [] });
  useSidebarDataStore.setState({ data: EMPTY_SIDEBAR_DATA });
  useScanUiStore.setState({
    starting: false,
    finding: false,
    missing: null,
    findError: null,
  });
});

afterEach(() => {
  setScanBackend(null);
  setToolsBackend(null);
});

describe("runRescan", () => {
  it("queues the scan and confirms with a toast naming the folders", async () => {
    const backend = fakeBackend();
    setScanBackend(backend);
    await runRescan();
    expect(backend.rescan).toHaveBeenCalledTimes(1);
    expect(useToastStore.getState().toasts.map((toast) => toast.message)).toEqual([
      "Scanning 2 folders",
    ]);
    expect(useScanUiStore.getState().starting).toBe(false);
  });

  it("says so when there is no folder to scan", async () => {
    setScanBackend(
      fakeBackend({
        rescan: () => Promise.resolve({ job_id: 1, sources: 0, media_info: "ready" as const }),
      }),
    );
    await runRescan();
    expect(useToastStore.getState().toasts[0]?.message).toBe(
      "Add a library folder first, then scan.",
    );
  });

  it("reports a failure in the notification centre, not in a dialog", async () => {
    setScanBackend(
      fakeBackend({
        rescan: () => Promise.reject(new ScanRequestError("storage", "The library is locked.")),
      }),
    );
    await runRescan();
    const [note] = useNotificationStore.getState().items;
    expect(note?.severity).toBe("error");
    expect(note?.title).toBe("Could not start the scan");
    expect(note?.detail).toBe("The library is locked.");
    expect(useScanUiStore.getState().starting).toBe(false);
  });

  it("does not start a second scan while one is being queued", async () => {
    let finish: () => void = () => undefined;
    const backend = fakeBackend({
      rescan: vi.fn(
        () =>
          new Promise<RescanStarted>((resolve) => {
            finish = () => resolve({ job_id: 1, sources: 1, media_info: "ready" as const });
          }),
      ),
    });
    setScanBackend(backend);
    const first = runRescan();
    await runRescan();
    expect(backend.rescan).toHaveBeenCalledTimes(1);
    finish();
    await first;
  });

  it("explains that scanning needs the desktop app in a plain browser", async () => {
    setScanBackend({ ...fakeBackend(), available: false });
    await runRescan();
    expect(useToastStore.getState().toasts[0]?.message).toBe(
      "Scanning is available in the desktop app.",
    );
  });
});

function Shell({ client, children }: { client: QueryClient; children: React.ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("the live source health", () => {
  // qa2-02: Home's size and "offline, not counted" follow a folder that goes away or comes back.
  it("marks Home and the missing review stale when a folder goes offline or comes back", async () => {
    let online = true;
    setScanBackend(
      fakeBackend({ sourceHealth: () => Promise.resolve([health({ id: 1, online })]) }),
    );
    const client = new QueryClient();
    renderHook(() => useSidebarSources(), {
      wrapper: ({ children }) => <Shell client={client}>{children}</Shell>,
    });
    await waitFor(() => expect(client.getQueryData(SOURCE_HEALTH_KEY)).toBeDefined());
    client.setQueryData(["home", "size"], { counted: 1 });
    client.setQueryData(["library-ops", "review"], { rows: [], kept: 0 });
    online = false;
    await act(() => client.invalidateQueries({ queryKey: SOURCE_HEALTH_KEY }));
    await waitFor(() => expect(client.getQueryState(["home", "size"])?.isInvalidated).toBe(true));
    expect(client.getQueryState(["library-ops", "review"])?.isInvalidated).toBe(true);
  });

  it("fills the sidebar folder list and marks a disconnected drive offline", async () => {
    setScanBackend(
      fakeBackend({
        sourceHealth: () =>
          Promise.resolve([
            health({ id: 1, path: "D:\\Media\\Movies", titles: 49, missing: 3 }),
            health({ id: 2, kind: "tv", path: "E:\\Shows", online: false, titles: 23 }),
          ]),
      }),
    );
    const client = new QueryClient();
    renderHook(() => useSidebarSources(), {
      wrapper: ({ children }) => <Shell client={client}>{children}</Shell>,
    });
    await waitFor(() => expect(useSidebarDataStore.getState().data.sources).toHaveLength(2));
    expect(useSidebarDataStore.getState().data.sources).toEqual([
      {
        id: 1,
        path: "D:\\Media\\Movies",
        label: "Movies",
        online: true,
        count: 49,
        missing: 3,
      },
      { id: 2, path: "E:\\Shows", label: "Shows", online: false, count: 23, missing: 0 },
    ]);
  });

  it("reads the folders again when a background job ends", async () => {
    const sourceHealth = vi.fn(() => Promise.resolve([health()]));
    setScanBackend(fakeBackend({ sourceHealth }));
    useJobProgressStore.setState({ jobs: [] });
    const client = new QueryClient();
    renderHook(
      () => {
        useSidebarSources();
        useRefreshAfterJobs();
      },
      { wrapper: ({ children }) => <Shell client={client}>{children}</Shell> },
    );
    await waitFor(() => expect(sourceHealth).toHaveBeenCalledTimes(1));
    const job: JobProgress = {
      id: 3,
      name: "Scan folders",
      lane: "disk",
      state: "running",
      priority: "normal",
      total: 2,
      done: 1,
      failed: 0,
      current: [],
      throughput_per_sec: null,
      eta_secs: null,
    } as unknown as JobProgress;
    act(() => useJobProgressStore.setState({ jobs: [job] }));
    expect(sourceHealth).toHaveBeenCalledTimes(1);
    act(() => useJobProgressStore.setState({ jobs: [{ ...job, state: "completed" }] }));
    await waitFor(() => expect(sourceHealth).toHaveBeenCalledTimes(2));
    expect(client.getQueryState(SOURCE_HEALTH_KEY)?.isInvalidated).toBeFalsy();
  });

  // Home's size, runtime, recent strip and breakdowns are read again after a job, never kept for
  // their five-minute stale time.
  it("marks Home and the missing review stale when a background job ends", () => {
    setScanBackend(fakeBackend());
    useJobProgressStore.setState({ jobs: [] });
    const client = new QueryClient();
    client.setQueryData(["home", "size"], { counted: 1 });
    client.setQueryData(["home", "recent"], []);
    client.setQueryData(["library-ops", "review"], { rows: [], kept: 0 });
    renderHook(() => useRefreshAfterJobs(), {
      wrapper: ({ children }) => <Shell client={client}>{children}</Shell>,
    });
    const job = {
      id: 4,
      name: "Scan folders",
      lane: "disk",
      state: "completed",
      priority: "normal",
      total: 2,
      done: 2,
      failed: 0,
      current: [],
      throughput_per_sec: null,
      eta_secs: null,
    } as unknown as JobProgress;
    act(() => useJobProgressStore.setState({ jobs: [job] }));
    expect(client.getQueryState(["home", "size"])?.isInvalidated).toBe(true);
    expect(client.getQueryState(["home", "recent"])?.isInvalidated).toBe(true);
    expect(client.getQueryState(["library-ops", "review"])?.isInvalidated).toBe(true);
  });
});

describe("titles during a scan", () => {
  it("reads the library again each time a folder of the running scan is done", () => {
    setScanBackend(fakeBackend());
    useJobProgressStore.setState({ jobs: [] });
    const client = new QueryClient();
    const window = ["library", "window", "movies", 0];
    client.setQueryData(window, { total: 0, items: [] });
    renderHook(() => useRefreshDuringScan(), {
      wrapper: ({ children }) => <Shell client={client}>{children}</Shell>,
    });
    const scan = (done: number) =>
      ({
        id: 9,
        name: "Scanning 2 folders",
        lane: "disk",
        state: "running",
        priority: "normal",
        total: 2,
        done,
        failed: 0,
        current: [],
        throughput_per_sec: null,
        eta_secs: null,
      }) as unknown as JobProgress;
    act(() => useJobProgressStore.setState({ jobs: [scan(0)] }));
    expect(client.getQueryState(window)?.isInvalidated).toBe(false);
    // The first folder is read: its titles list now, not when the whole scan ends.
    act(() => useJobProgressStore.setState({ jobs: [scan(1)] }));
    expect(client.getQueryState(window)?.isInvalidated).toBe(true);
  });

  it("reads the library at most once every two seconds while folders keep finishing", () => {
    vi.useFakeTimers();
    try {
      setScanBackend(fakeBackend());
      useJobProgressStore.setState({ jobs: [] });
      const client = new QueryClient();
      const invalidate = vi.spyOn(client, "invalidateQueries");
      renderHook(() => useRefreshDuringScan(), {
        wrapper: ({ children }) => <Shell client={client}>{children}</Shell>,
      });
      const libraryReads = () =>
        invalidate.mock.calls.filter(
          ([filters]) => filters?.queryKey?.length === 1 && filters.queryKey[0] === "library",
        ).length;
      const scan = (done: number) =>
        ({
          id: 11,
          name: "Scanning 50 folders",
          lane: "disk",
          state: "running",
          priority: "normal",
          total: 50,
          done,
          failed: 0,
          current: [],
          throughput_per_sec: null,
          eta_secs: null,
        }) as unknown as JobProgress;
      act(() => useJobProgressStore.setState({ jobs: [scan(1)] }));
      expect(libraryReads()).toBe(1);
      // Ten folders in one second: no read yet, one when the pause is over.
      for (let done = 2; done <= 11; done += 1) {
        act(() => {
          vi.advanceTimersByTime(100);
          useJobProgressStore.setState({ jobs: [scan(done)] });
        });
      }
      expect(libraryReads()).toBe(1);
      act(() => vi.advanceTimersByTime(SCAN_REFRESH_MS));
      expect(libraryReads()).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("the media settings", () => {
  it("are in the library section, off by default", () => {
    for (const id of [
      "media.store-mediainfo-xml",
      "media.runtime-from-media",
      "media.detect-aspect-ratio",
    ]) {
      const def = settingDef(id);
      expect(def?.section, id).toBe("library");
      expect(def?.default, id).toBe(false);
      expect(def?.title, id).not.toBe("");
    }
    // Scanning at start and on folder changes is always on: there is no setting for either.
    expect(settingDef("library.scan-on-startup")).toBeUndefined();
    expect(settingDef("library.watch-folders")).toBeUndefined();
  });
});
