import { useState } from "react";

import { Popover } from "../../components/Popover";
import { ProgressCard } from "../../components/ProgressCard";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { RunningEntryRow } from "./RunningEntryRow";
import { useRunningEntries } from "./runningEntries";

interface SidebarProgressProps {
  collapsed: boolean;
}

/**
 * The progress card at the foot of the sidebar (spec `app-shell` "Progress card"): while work
 * runs it names the job started first in bold ("Scraping 3 of 25", a scan "Scanning 312 of 707
 * titles", "+1 more" when others run), the percentage muted and a thin gold bar; collapsed it is
 * a ring. Choosing it opens a small floating list of the running jobs, of a move and of a rename
 * in progress, with Pause and Cancel where the work has them. When nothing runs it is gone.
 */
export function SidebarProgress({ collapsed }: SidebarProgressProps) {
  const entries = useRunningEntries();
  const [open, setOpen] = useState(false);
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
          <RunningEntryRow key={entry.key} entry={entry} />
        ))}
      </ul>
    </Popover>
  );
}
