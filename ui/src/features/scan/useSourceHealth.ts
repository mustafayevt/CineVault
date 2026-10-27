import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import type { JobProgress } from "../../bindings";
import { isScanJob } from "../jobs/jobNames";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { LIBRARY_COUNTS_KEY } from "../library/useLibraryCounts";
import { currentSourcesBackend } from "../library-sources/sourcesBackend";
import { defaultSourceLabel } from "../sidebar/sourceNames";
import { useSidebarDataStore } from "../sidebar/sidebarData";
import { currentScanBackend } from "./scanBackend";

/** Query key of the library folders' live state. */
export const SOURCE_HEALTH_KEY = ["sources", "health"] as const;

/** Query key of the library folders with their labels. */
export const SOURCE_LABELS_KEY = ["sources", "labels"] as const;

/** How often the folders are probed while the window is open. */
const HEALTH_REFRESH_MS = 15_000;

/**
 * Keeps the sidebar's folder list live: each library folder shows its label (the folder's last
 * name until the user changes it), a green dot while it answers and a hollow one while the drive
 * is disconnected, with the number of titles it holds. The probe
 * repeats every few seconds, and again whenever a background job ends (a scan adds titles, a
 * drive that came back is noticed by the next scan). Mount it once, in the shell.
 */
export function useSidebarSources(): void {
  const client = useQueryClient();
  const reachable = useRef<string | null>(null);
  const { data } = useQuery({
    queryKey: SOURCE_HEALTH_KEY,
    queryFn: () => currentScanBackend().sourceHealth(),
    staleTime: HEALTH_REFRESH_MS / 2,
    refetchInterval: HEALTH_REFRESH_MS,
  });
  const { data: labelled } = useQuery({
    queryKey: SOURCE_LABELS_KEY,
    queryFn: () => currentSourcesBackend().list(),
    staleTime: HEALTH_REFRESH_MS / 2,
    refetchInterval: HEALTH_REFRESH_MS,
  });
  // A folder that goes offline or comes back changes what Home counts (the size of the online
  // folders, "N offline, not counted"), the missing review and the library rows.
  useEffect(() => {
    if (data === undefined) return;
    const now = data.map((source) => `${source.id}:${source.online ? 1 : 0}`).join(",");
    const before = reachable.current;
    reachable.current = now;
    if (before === null || before === now) return;
    void client.invalidateQueries({ queryKey: ["home"] });
    void client.invalidateQueries({ queryKey: ["library-ops", "review"] });
    void client.invalidateQueries({ queryKey: ["library"] });
    void client.invalidateQueries({ queryKey: ["lifecycle"] });
  }, [data, client]);
  useEffect(() => {
    if (data === undefined) return;
    const labels = new Map((labelled ?? []).map((source) => [source.id, source.label]));
    const { data: current, setData } = useSidebarDataStore.getState();
    setData({
      ...current,
      sources: data.map((source) => ({
        id: source.id,
        path: source.path,
        label: labels.get(source.id) ?? defaultSourceLabel(source.path),
        online: source.online,
        count: source.titles,
        missing: source.missing,
      })),
    });
  }, [data, labelled]);
}

/**
 * Shows a scan's titles as they arrive: each time a folder of a running scan is done, the
 * library views, the counts and the folder health are read again, so the titles of a folder
 * that was read (with their local NFO and artwork) list before the whole scan and any online
 * lookup are over. The reads happen at most once every {@link SCAN_REFRESH_MS} (the first one at
 * once, a later one when the pause is over); the end of the scan is covered by
 * {@link useRefreshAfterJobs}. It listens to the progress store without re-rendering. Mount it
 * once, in the shell.
 */
export function useRefreshDuringScan(): void {
  const client = useQueryClient();
  useEffect(() => {
    let seen: ReadonlyMap<number, number> = new Map();
    let last = Number.NEGATIVE_INFINITY;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = () => {
      timer = undefined;
      last = Date.now();
      void client.invalidateQueries({ queryKey: ["library"] });
      void client.invalidateQueries({ queryKey: LIBRARY_COUNTS_KEY });
      void client.invalidateQueries({ queryKey: SOURCE_HEALTH_KEY });
    };
    const check = (jobs: readonly JobProgress[]) => {
      const scans = jobs.filter((job) => isScanJob(job) && job.state === "running");
      const advanced = scans.some((job) => job.done > (seen.get(job.id) ?? 0));
      seen = new Map(scans.map((job) => [job.id, job.done]));
      if (!advanced || timer !== undefined) return;
      const wait = last + SCAN_REFRESH_MS - Date.now();
      if (wait <= 0) refresh();
      else timer = setTimeout(refresh, wait);
    };
    check(useJobProgressStore.getState().jobs);
    const unsubscribe = useJobProgressStore.subscribe((state, previous) => {
      if (state.jobs !== previous.jobs) check(state.jobs);
    });
    return () => {
      unsubscribe();
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [client]);
}

/** The shortest time between two library reads while a scan lists titles. */
export const SCAN_REFRESH_MS = 2_000;

/**
 * Refreshes what a finished job changes: when any background job ends, the library views, the
 * sidebar counts, Home (its size, runtime, recent strip and breakdowns), the missing and
 * duplicate review and the folder health are read again. It listens to the progress store
 * without re-rendering. Mount it once, in the shell.
 */
export function useRefreshAfterJobs(): void {
  const client = useQueryClient();
  useEffect(() => {
    let finished: ReadonlySet<number> = new Set();
    const check = (jobs: readonly JobProgress[]) => {
      const ended = jobs.filter(
        (job) => job.state === "completed" || job.state === "failed" || job.state === "cancelled",
      );
      const fresh = ended.filter((job) => !finished.has(job.id));
      if (fresh.length === 0) return;
      finished = new Set(ended.map((job) => job.id));
      void client.invalidateQueries({ queryKey: ["library"] });
      void client.invalidateQueries({ queryKey: LIBRARY_COUNTS_KEY });
      void client.invalidateQueries({ queryKey: ["lifecycle"] });
      void client.invalidateQueries({ queryKey: SOURCE_HEALTH_KEY });
      void client.invalidateQueries({ queryKey: ["home"] });
      void client.invalidateQueries({ queryKey: ["library-ops", "review"] });
    };
    check(useJobProgressStore.getState().jobs);
    return useJobProgressStore.subscribe((state, previous) => {
      if (state.jobs !== previous.jobs) check(state.jobs);
    });
  }, [client]);
}
