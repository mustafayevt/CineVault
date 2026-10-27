import { t } from "../../i18n/catalog";
import type { HistoryEntry } from "./historyBackend";

/** Entries read per request; "Show older entries" reads the next ones. */
export const PAGE_SIZE = 40;

/** One day of the list. */
export interface Day {
  key: string;
  label: string;
  entries: HistoryEntry[];
}

/** The entries (newest first) grouped by day: Today, Yesterday, then dates. */
export function groupByDay(
  entries: readonly HistoryEntry[],
  now: Date,
  dateText: (ms: number) => string,
): Day[] {
  const startOf = (ms: number) => {
    const day = new Date(ms);
    day.setHours(0, 0, 0, 0);
    return day.getTime();
  };
  const today = startOf(now.getTime());
  const yesterday = startOf(today - 12 * 3_600_000);
  const days: Day[] = [];
  for (const entry of entries) {
    const at = entry.at ?? 0;
    const key = String(startOf(at));
    let day = days.at(-1);
    if (day?.key !== key) {
      const start = Number(key);
      day = {
        key,
        label:
          start === today
            ? t("history.day.today")
            : start === yesterday
              ? t("history.day.yesterday")
              : dateText(at),
        entries: [],
      };
      days.push(day);
    }
    day.entries.push(entry);
  }
  return days;
}
