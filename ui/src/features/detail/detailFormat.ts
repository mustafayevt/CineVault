import { t } from "../../i18n/catalog";
import type { MovieDetail } from "./detailBackend";

/** `2h 46m`, `2h`, `46 min`; empty when the runtime is unknown. */
export function runtimeLabel(minutes: number): string {
  if (minutes <= 0) return "";
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return t("detail.runtime.minutes", { minutes: rest });
  return rest === 0
    ? t("detail.runtime.hours", { hours })
    : t("detail.runtime.hoursMinutes", { hours, minutes: rest });
}

/** The line under the title: `2024 · PG-13 · 2h 46m · Science Fiction, Adventure`. */
export function metaLine(detail: MovieDetail): string {
  return [
    detail.year > 0 ? String(detail.year) : "",
    detail.certification,
    runtimeLabel(detail.runtime_minutes),
    detail.genres.join(", "),
  ]
    .filter((part) => part !== "")
    .join(" · ");
}

const DAY_MS = 86_400_000;

/** `today`, `3 days ago`, `2 months ago`, `last year`; empty when the date is unknown. */
export function addedAgo(addedMs: number | null, now: number, locale: string): string {
  if (addedMs === null || addedMs <= 0) return "";
  const days = Math.max(0, Math.floor((now - addedMs) / DAY_MS));
  const relative = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (days < 30) return relative.format(-days, "day");
  if (days < 365) return relative.format(-Math.floor(days / 30), "month");
  return relative.format(-Math.floor(days / 365), "year");
}

/** The language a code names (`tr` is Turkish), or the text as it is when it is no code. */
export function languageName(code: string, locale: string): string {
  const text = code.trim();
  if (text === "") return "";
  if (!/^[a-z]{2,3}(?:[-_][A-Za-z]{2,4})?$/i.test(text)) return text;
  try {
    return new Intl.DisplayNames([locale], { type: "language" }).of(text.replace("_", "-")) ?? text;
  } catch {
    return text;
  }
}

/** Two letters for a person without a photo. */
export function initials(name: string): string {
  const words = name.split(/\s+/).filter((word) => word !== "");
  return words
    .slice(0, 2)
    .map((word) => Array.from(word)[0] ?? "")
    .join("")
    .toUpperCase();
}
