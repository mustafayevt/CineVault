import { Pause, Play, X } from "lucide-react";
import { useState } from "react";
import { useShallow } from "zustand/react/shallow";

import type { JobProgress } from "../../bindings";
import { IconButton } from "../../components/IconButton";
import { Popover } from "../../components/Popover";
import { ProgressBar } from "../../components/ProgressBar";
import { ProgressCard } from "../../components/ProgressCard";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { useJobProgressStore } from "../jobs/jobProgressStore";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { useMoveToStore } from "../move-to/moveToStore";
import type { RunningMove } from "../move-to/moveToStore";
import { currentJobControls } from "./jobControls";
import { jobLine, jobPercent, moveLine, visibleJobs } from "./progressLines";

/** One line of the running work: a backend job, or the move the user started. */
interface Entry {
  key: string;
  name: string;
  /** The card's bold line when this is the first entry. */
  line: string;
  percent: number;
  /** "3 of 25 · 12%", or null while the size is unknown. */
  count: string | null;
  current: string | undefined;
  paused: boolean;
  setPaused: (paused: boolean) => void;
  cancel: () => void;
}

function jobEntry(job: JobProgress): Entry {
  const percent = jobPercent(job);
  return {
    key: `job-${job.id}`,
    name: job.name,
    line: jobLine(job),
    percent,
    count:
      job.total > 0
        ? t("shell.progress.count", {
            done: job.done,
            total: job.total,
            percent: Math.round(percent),
          })
        : null,
    current: job.current[0],
    paused: job.state === "paused",
    setPaused: (paused) => void currentJobControls().setPaused(job.id, paused),
    cancel: () => void currentJobControls().cancel(job.id),
  };
}

/** A move is not a backend job: its progress and its Pause and Cancel come from the move itself. */
function moveEntry(move: RunningMove): Entry {
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
    setPaused: (paused) => {
      const store = useMoveToStore.getState();
      if (store.running !== null) store.setRunning({ ...store.running, paused });
      void currentLibraryOpsBackend().controlMove(paused ? "pause" : "resume");
    },
    cancel: () => void currentLibraryOpsBackend().controlMove("cancel"),
  };
}

function EntryRow({ entry }: { entry: Entry }) {
  return (
    <li className="flex flex-col gap-1.5 px-2 py-2">
      <div className="flex items-center gap-2">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-(length:--fs-13) font-(weight:--fw-600) text-text">
            {entry.name}
          </span>
          <span className="truncate text-(length:--fs-12) text-text-muted tabular-nums">
            {entry.count ?? t("shell.progress.working")}
            {entry.current === undefined || entry.current === "" ? "" : ` · ${entry.current}`}
          </span>
        </div>
        <IconButton
          size="sm"
          icon={entry.paused ? Play : Pause}
          aria-label={
            entry.paused
              ? t("shell.progress.resume", { name: entry.name })
              : t("shell.progress.pause", { name: entry.name })
          }
          onClick={() => entry.setPaused(!entry.paused)}
        />
        <IconButton
          size="sm"
          icon={X}
          aria-label={t("shell.progress.cancel", { name: entry.name })}
          onClick={entry.cancel}
        />
      </div>
      <ProgressBar label={entry.name} percent={entry.percent} />
    </li>
  );
}

interface SidebarProgressProps {
  collapsed: boolean;
}

/**
 * The progress card at the foot of the sidebar (spec `app-shell` "Progress card"): while work
 * runs it names the job started first in bold ("Scraping 3 of 25", "+1 more" when others run),
 * the percentage muted and a thin gold bar; collapsed it is a ring. Choosing it opens a small
 * floating list of the running jobs and of a move in progress, with Pause and Cancel for each.
 * When nothing runs it is gone.
 */
export function SidebarProgress({ collapsed }: SidebarProgressProps) {
  const jobs = useJobProgressStore(useShallow((state) => visibleJobs(state.jobs)));
  const move = useMoveToStore((state) => state.running);
  const [open, setOpen] = useState(false);
  const entries = [...jobs.map(jobEntry), ...(move === null ? [] : [moveEntry(move)])];
  const first = entries[0];
  if (first === undefined) return null;
  const others = entries.length - 1;
  const title = first.line;
  const more = others > 0 ? t("shell.progress.more", { count: others }) : undefined;
  const spoken = more === undefined ? title : `${title}, ${more}`;
  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      side={collapsed ? "right" : "top"}
      align={collapsed ? "end" : "start"}
      aria-label={t("shell.progress.list")}
      className="w-80 p-1"
      trigger={
        <button
          type="button"
          aria-label={t("shell.progress.open", { work: spoken })}
          data-testid="sidebar-progress"
          className={cx(
            "group cv-hoverable block rounded-(--r-6) text-left",
            collapsed
              ? "mx-auto hover:bg-surface-floating-hover active:bg-surface-pressed"
              : "w-full",
          )}
        >
          <ProgressCard
            title={title}
            more={more}
            percent={first.percent}
            collapsed={collapsed}
            className={
              collapsed
                ? undefined
                : "cv-hoverable group-hover:bg-surface-floating-hover group-active:bg-surface-pressed"
            }
          />
        </button>
      }
    >
      <ul className="m-0 flex list-none flex-col p-0">
        {entries.map((entry) => (
          <EntryRow key={entry.key} entry={entry} />
        ))}
      </ul>
    </Popover>
  );
}
