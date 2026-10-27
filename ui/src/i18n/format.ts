import { t } from "./catalog";
import type { MessageKey } from "./catalog";
import { dateFormat, numberFormat } from "./locale";
import type { SizeBase } from "./locale";

/** A moment: a `Date`, epoch milliseconds or an ISO 8601 string. */
export type Moment = Date | number | string;

export interface SizeOptions {
  /** Unit system. Defaults to the one in effect (`base10`). */
  base?: SizeBase;
  /** Locale for the digits. Defaults to the one in effect. */
  locale?: string;
}

const UNIT_COUNT = 6;

/** What the size unit steps by: 1000 for GB, 1024 for GiB. */
function sizeStep(base: SizeBase): number {
  return base === "base2" ? 1024 : 1000;
}

/** The symbol for unit `index` (0 = bytes). */
function sizeUnit(base: SizeBase, index: number): string {
  const clamped = Math.min(Math.max(index, 0), UNIT_COUNT - 1);
  return t(`size.${base}.${clamped}` as MessageKey);
}

/** More digits for small numbers: 4.7 GiB, 47 GiB, 470 GiB. */
function fractionDigits(value: number): number {
  if (value < 10) return 2;
  if (value < 100) return 1;
  return 0;
}

function roundTo(value: number, digits: number): number {
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

/**
 * A file size with its unit in the chosen base: `4.7 GB` (base 10) or `4.38 GiB` (base 2) for the
 * same bytes. Digits follow the locale (`4,7 GB` in German). Invalid input gives the "Unknown size" text.
 */
export function formatBytes(bytes: number, locale: string, base: SizeBase): string {
  if (!Number.isFinite(bytes) || bytes < 0) return t("common.unknownSize");
  const step = sizeStep(base);
  let exponent = 0;
  let value = bytes;
  while (value >= step && exponent < UNIT_COUNT - 1) {
    value /= step;
    exponent += 1;
  }
  let digits = exponent === 0 ? 0 : fractionDigits(value);
  // 999.99 kB rounds to 1000: show it as 1 MB instead.
  if (exponent < UNIT_COUNT - 1 && roundTo(value, digits) >= step) {
    value /= step;
    exponent += 1;
    digits = fractionDigits(value);
  }
  const text = numberFormat(locale, {
    minimumFractionDigits: 0,
    maximumFractionDigits: digits,
  }).format(value);
  return t("size.value", { value: text, unit: sizeUnit(base, exponent) });
}

function listFormat(locale: string, type: "unit" | "conjunction"): Intl.ListFormat {
  return new Intl.ListFormat(locale, { style: "long", type });
}

function toDate(moment: Moment): Date {
  return moment instanceof Date ? moment : new Date(moment);
}

function safe(moment: Moment, format: (date: Date) => string): string {
  const date = toDate(moment);
  return Number.isNaN(date.getTime()) ? "" : format(date);
}

/** The formatting functions for one locale and size base. */
export interface Formatters {
  locale: string;
  sizeBase: SizeBase;
  /** `1,234,567` or `1.234.567`. */
  number: (value: number) => string;
  /** A fraction as a percentage: 0.25 becomes `25%`. */
  percent: (fraction: number) => string;
  /** A file size from bytes in the selected base: `4.7 GB` or `4.38 GiB`. */
  size: (bytes: number) => string;
  /** `20 Oct 2026`. Empty for an invalid date. */
  date: (moment: Moment, options?: { timeZone?: string }) => string;
  /** `4:05 PM` or `16:05`. Empty for an invalid date. */
  time: (moment: Moment, options?: { timeZone?: string }) => string;
  /** Date and time together. Empty for an invalid date. */
  dateTime: (moment: Moment, options?: { timeZone?: string }) => string;
  /** Short phrases in a row: `a, b, c`. */
  list: (items: readonly string[]) => string;
  /** Names joined with the locale's word for and: `a and b`. */
  andList: (items: readonly string[]) => string;
}

/** Builds the formatters for a locale (a BCP 47 tag) and size base. */
export function createFormatters(locale: string, sizeBase: SizeBase): Formatters {
  return {
    locale,
    sizeBase,
    number: (value) => numberFormat(locale).format(value),
    percent: (fraction) =>
      numberFormat(locale, { style: "percent", maximumFractionDigits: 0 }).format(fraction),
    size: (bytes) => formatBytes(bytes, locale, sizeBase),
    date: (moment, options) =>
      safe(moment, (date) => dateFormat(locale, { dateStyle: "medium", ...options }).format(date)),
    time: (moment, options) =>
      safe(moment, (date) => dateFormat(locale, { timeStyle: "short", ...options }).format(date)),
    dateTime: (moment, options) =>
      safe(moment, (date) =>
        dateFormat(locale, { dateStyle: "medium", timeStyle: "short", ...options }).format(date),
      ),
    list: (items) => listFormat(locale, "unit").format(items),
    andList: (items) => listFormat(locale, "conjunction").format(items),
  };
}
