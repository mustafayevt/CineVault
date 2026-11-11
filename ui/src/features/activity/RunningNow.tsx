import { t } from "../../i18n/catalog";
import { RunningEntryRow } from "../shell/RunningEntryRow";
import { useRunningEntries } from "../shell/runningEntries";

/**
 * The top of Activity while work runs (spec `operation-history` "Activity page"): every job, move
 * and rename as the sidebar's Running work list shows it, with the same lines, bars and controls,
 * so a scan reads "Scanning 312 of 707 titles" here too. Gone when nothing runs.
 */
export function RunningNow() {
  const entries = useRunningEntries();
  if (entries.length === 0) return null;
  return (
    <section
      aria-labelledby="running-now-heading"
      data-testid="running-now"
      className="flex shrink-0 flex-col border-b border-solid border-line-section px-6 pt-3 pb-3 @max-[640px]/pane:px-4"
    >
      <div className="flex h-8 shrink-0 items-center gap-2">
        <h2
          id="running-now-heading"
          className="m-0 text-(length:--fs-13) font-(weight:--fw-600) text-text"
        >
          {t("activity.running")}
        </h2>
      </div>
      <ul className="m-0 mt-1.5 flex max-h-48 list-none flex-col gap-0.5 overflow-y-auto rounded-(--r-10) border border-solid border-line-edge bg-surface-card p-1">
        {entries.map((entry) => (
          <RunningEntryRow key={entry.key} entry={entry} />
        ))}
      </ul>
    </section>
  );
}
