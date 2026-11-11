import { useShallow } from "zustand/react/shallow";

import type { JobProgress } from "../../bindings";
import { t } from "../../i18n/catalog";
import { serverClientId } from "../../lib/ipc";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { useMoveToStore } from "../move-to/moveToStore";
import type { RunningMove } from "../move-to/moveToStore";
import { useRenameRunStore } from "../rename/runRename";
import type { RunningRename } from "../rename/runRename";
import { currentJobControls } from "./jobControls";
import {
  jobCountText,
  jobCurrent,
  jobLine,
  jobPercent,
  moveLine,
  visibleJobs,
} from "./progressLines";

/** One line of the running work: a backend job, the move or the rename the user started. */
export interface RunningEntry {
  key: string;
  name: string;
  /** The card's bold line when this is the first entry. */
  line: string;
  percent: number;
  /** "3 of 25 · 12%", or null while the size is unknown. */
  count: string | null;
  current: string | undefined;
  paused: boolean;
  otherSession: boolean;
  /** Absent for work that cannot be paused (a rename). */
  setPaused?: (paused: boolean) => void;
  /** Absent for work that cannot be cancelled (a rename). */
  cancel?: () => void;
}

function jobEntry(job: JobProgress): RunningEntry {
  return {
    key: `job-${job.id}`,
    name: job.name,
    line: jobLine(job),
    percent: jobPercent(job),
    count: jobCountText(job),
    current: jobCurrent(job),
    paused: job.state === "paused",
    otherSession:
      job.started_by != null && job.started_by !== "" && job.started_by !== serverClientId(),
    setPaused: (paused) => void currentJobControls().setPaused(job.id, paused),
    cancel: () => void currentJobControls().cancel(job.id),
  };
}

/** A move is not a backend job: its progress and its Pause and Cancel come from the move itself. */
function moveEntry(move: RunningMove): RunningEntry {
  const name = t("move.progress.name", { label: move.label, target: move.target });
  return {
    key: "move",
    name,
    line: moveLine(name, move.paused),
    percent: move.percent,
    count:
      move.speed === undefined || move.paused
        ? `${Math.round(move.percent)}%`
        : `${Math.round(move.percent)}% · ${move.speed}`,
    current: move.current,
    paused: move.paused,
    otherSession: false,
    setPaused: (paused) => {
      const store = useMoveToStore.getState();
      if (store.running !== null) store.setRunning({ ...store.running, paused });
      void currentLibraryOpsBackend().controlMove(paused ? "pause" : "resume");
    },
    cancel: () => void currentLibraryOpsBackend().controlMove("cancel"),
  };
}

/** A rename runs title by title and is short: its progress, without Pause or Cancel. */
function renameEntry(rename: RunningRename): RunningEntry {
  const percent = rename.total > 0 ? Math.min(100, (rename.done / rename.total) * 100) : 0;
  return {
    key: "rename",
    name: rename.name,
    line: t("renamePage.progress.line", { done: rename.done, total: rename.total }),
    percent,
    count: t("shell.progress.count", {
      done: rename.done,
      total: rename.total,
      percent: Math.round(percent),
    }),
    current: rename.title === "" ? undefined : rename.title,
    paused: false,
    otherSession: false,
  };
}

/** The running work, oldest first: the jobs the user started, then a move and a rename. */
export function useRunningEntries(): RunningEntry[] {
  const jobs = useJobProgressStore(useShallow((state) => visibleJobs(state.jobs)));
  const move = useMoveToStore((state) => state.running);
  const rename = useRenameRunStore((state) => state.running);
  return [
    ...jobs.map(jobEntry),
    ...(move === null ? [] : [moveEntry(move)]),
    ...(rename === null ? [] : [renameEntry(rename)]),
  ];
}
