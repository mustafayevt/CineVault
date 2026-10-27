import { t } from "../../i18n/catalog";

/**
 * "Up to date · checked today at 09:12" (spec `app-shell` "Version and update check in
 * Settings"): the answer of the last check and when it ended, as a day word or date and a time.
 * `head` replaces "Up to date" when the check found an update or failed.
 */
export function lastCheckText(
  checkedAt: number | null,
  now: number,
  locale: string,
  head: string = t("settingsPage.updates.upToDate"),
): string {
  if (checkedAt === null) return t("settingsPage.updates.notChecked");
  const time = new Intl.DateTimeFormat(locale, { timeStyle: "short" }).format(checkedAt);
  const sameDay = new Date(checkedAt).toDateString() === new Date(now).toDateString();
  const when = sameDay
    ? t("settingsPage.updates.today", { time })
    : t("settingsPage.updates.on", {
        date: new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(checkedAt),
        time,
      });
  return `${head} · ${when}`;
}
