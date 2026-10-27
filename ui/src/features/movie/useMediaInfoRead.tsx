import { useQueryClient } from "@tanstack/react-query";
import type { QueryKey } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { t } from "../../i18n/catalog";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { isLiveJob, isMediaInfoJob } from "../jobs/jobNames";
import { currentScanBackend } from "../scan/scanBackend";
import { currentToolsBackend } from "../tools/toolsBackend";

/** Why the video details could not be read. */
type ReadProblem = { kind: "missing" } | { kind: "failed"; message: string };

/** How long after a read the details are fetched once more: they are read in the background. */
const REFRESH_AFTER_MS = 3000;

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface MediaInfoRead {
  /** Reads the video details the scan has not read yet. */
  readNow: () => void;
  /** Busy: the request is on its way, or the read it started is still running. */
  reading: boolean;
  /** What to show beside "Read now" when it cannot do its job: why, and a way to fix it. */
  note: ReactNode;
}

/**
 * "Read now": reads the pending video details and never fails silently. It stays busy while the
 * read it asked for runs (a large file on a hard disk takes a minute), and refreshes when it ends. When MediaInfo is not
 * installed it says so with a button that downloads it right there through the tools manager and
 * reads again; any other failure is written out. `keys` are the queries to refresh afterwards.
 */
export function useMediaInfoRead(keys: readonly QueryKey[]): MediaInfoRead {
  const queryClient = useQueryClient();
  const [reading, setReading] = useState(false);
  const [problem, setProblem] = useState<ReadProblem | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  // A read was asked for here, and the media-info job it started has not ended yet.
  const [waiting, setWaiting] = useState(false);
  const jobRunning = useJobProgressStore((state) =>
    state.jobs.some((job) => isLiveJob(job) && isMediaInfoJob(job)),
  );
  const seenRunning = useRef(false);
  const timer = useRef<number | undefined>(undefined);
  const keysRef = useRef(keys);
  useEffect(() => {
    keysRef.current = keys;
  });

  const refresh = useCallback(() => {
    for (const key of keysRef.current) void queryClient.invalidateQueries({ queryKey: key });
  }, [queryClient]);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (!waiting) return;
    if (jobRunning) {
      seenRunning.current = true;
      return;
    }
    if (seenRunning.current) {
      // The read ended: show what it found.
      seenRunning.current = false;
      setWaiting(false);
      refresh();
    }
  }, [waiting, jobRunning, refresh]);

  const read = useCallback(async () => {
    setProblem(null);
    try {
      const status = await currentScanBackend().readPendingMediaInfo();
      if (status === "missing") setProblem({ kind: "missing" });
      else setWaiting(true);
    } catch (error) {
      setProblem({ kind: "failed", message: messageOf(error) });
    }
    refresh();
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(refresh, REFRESH_AFTER_MS);
  }, [refresh]);

  const readNow = useCallback(() => {
    if (reading || (waiting && jobRunning)) return;
    setReading(true);
    void read().finally(() => setReading(false));
  }, [reading, waiting, jobRunning, read]);

  const install = useCallback(async () => {
    setPercent(0);
    setProblem(null);
    try {
      await currentToolsBackend().install("mediainfo", (progress) =>
        setPercent(
          progress.total > 0 ? Math.round((progress.downloaded / progress.total) * 100) : 0,
        ),
      );
    } catch (error) {
      setPercent(null);
      setProblem({ kind: "failed", message: messageOf(error) });
      return;
    }
    setPercent(null);
    setReading(true);
    await read().finally(() => setReading(false));
  }, [read]);

  let note: ReactNode = null;
  if (percent !== null) {
    note = (
      <p role="status" className="m-0 text-(length:--fs-12) text-text-2">
        {t("detail.tech.installing", { percent })}
      </p>
    );
  } else if (problem?.kind === "missing") {
    note = (
      <div role="alert" className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <p className="m-0 text-(length:--fs-12) text-warn">{t("detail.tech.mediainfoMissing")}</p>
        <Button size="sm" onClick={() => void install()}>
          {t("detail.tech.mediainfoDownload")}
        </Button>
      </div>
    );
  } else if (problem?.kind === "failed") {
    note = (
      <p role="alert" className="m-0 text-(length:--fs-12) text-warn">
        {t("detail.tech.readFailed", { message: problem.message })}
      </p>
    );
  }
  return { readNow, reading: reading || (waiting && jobRunning), note };
}
