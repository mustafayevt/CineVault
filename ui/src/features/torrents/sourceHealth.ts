import type { Status } from "../../components/status";
import { t } from "../../i18n/catalog";

/** The status tag and the words for the health the service reports for a source. */
export function healthTag(health: string): { status: Status; label: string } {
  switch (health) {
    case "working":
      return { status: "done", label: t("torrents.health.working") };
    case "needsSolver":
      return { status: "needs-key", label: t("torrents.health.needsSolver") };
    case "unreachable":
      return { status: "missing", label: t("torrents.health.unreachable") };
    case "error":
      return { status: "missing", label: t("torrents.health.error") };
    case "checking":
      return { status: "in-progress", label: t("torrents.health.checking") };
    case "off":
      return { status: "off", label: t("torrents.health.off") };
    default:
      return { status: "waiting", label: t("torrents.health.untested") };
  }
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "Checked 2 h ago": when a source was last asked. Empty while it never was. */
export function checkedText(checkedAt: number | null, now: number): string {
  if (checkedAt === null) return t("torrents.checked.never");
  const age = Math.max(0, now - checkedAt);
  if (age < MINUTE) return t("torrents.checked.now");
  if (age < HOUR) return t("torrents.checked.minutes", { count: Math.floor(age / MINUTE) });
  if (age < DAY) return t("torrents.checked.hours", { count: Math.floor(age / HOUR) });
  return t("torrents.checked.days", { count: Math.floor(age / DAY) });
}
