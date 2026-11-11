import { Pause, Play, X } from "lucide-react";

import { IconButton } from "../../components/IconButton";
import { ProgressBar } from "../../components/ProgressBar";
import { t } from "../../i18n/catalog";
import type { RunningEntry } from "./runningEntries";

/** One entry of the running work: its name, count and current item, its controls and its bar. */
export function RunningEntryRow({ entry }: { entry: RunningEntry }) {
  const { setPaused, cancel } = entry;
  return (
    <li className="flex flex-col gap-1.5 px-2 py-2" data-entry={entry.key}>
      <div className="flex items-center gap-2">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-(length:--fs-13) font-(weight:--fw-600) text-text">
            {entry.name}
          </span>
          <span className="truncate text-(length:--fs-12) text-text-muted tabular-nums">
            {entry.count ?? t("shell.progress.working")}
            {entry.current === undefined || entry.current === "" ? "" : ` · ${entry.current}`}
          </span>
          {entry.otherSession ? (
            <span className="text-(length:--fs-11) text-text-muted">
              {t("shell.progress.otherSession")}
            </span>
          ) : null}
        </div>
        {setPaused === undefined ? null : (
          <IconButton
            size="sm"
            icon={entry.paused ? Play : Pause}
            aria-label={
              entry.paused
                ? t("shell.progress.resume", { name: entry.name })
                : t("shell.progress.pause", { name: entry.name })
            }
            onClick={() => setPaused(!entry.paused)}
          />
        )}
        {cancel === undefined ? null : (
          <IconButton
            size="sm"
            icon={X}
            aria-label={t("shell.progress.cancel", { name: entry.name })}
            onClick={cancel}
          />
        )}
      </div>
      <ProgressBar label={entry.name} percent={entry.percent} />
    </li>
  );
}
