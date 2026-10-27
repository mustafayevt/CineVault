import { create } from "zustand";

/** Which unit system file sizes use: 1000-based (kB, MB, GB) or 1024-based (KiB, MiB, GiB). */
export type SizeBase = "base10" | "base2";

/**
 * The size units a person expects from their file manager: Windows Explorer counts in 1024-based
 * units, so a library there shows the same number as Explorer; Finder and others count in 1000s.
 */
export function defaultSizeBase(
  userAgent: string = typeof navigator === "undefined" ? "" : navigator.userAgent,
): SizeBase {
  return /Windows/i.test(userAgent) ? "base2" : "base10";
}

/** The locale preference that follows the operating system. */
export const LOCALE_SYSTEM = "system";

/** Used when the operating system reports nothing usable. */
const FALLBACK_LOCALE = "en-US";

interface LocaleState {
  /** `system` or a BCP 47 tag such as `de-DE`. */
  preference: string;
  sizeBase: SizeBase;
  setPreference: (preference: string) => void;
  setSizeBase: (sizeBase: SizeBase) => void;
}

/**
 * The formatting preferences in effect. The settings framework writes them (see
 * `features/settings/localeSettings.ts`); everything that formats a date, number or size reads
 * them from here, so the i18n layer never imports the settings store.
 */
export const useLocaleStore = create<LocaleState>((set) => ({
  preference: LOCALE_SYSTEM,
  sizeBase: defaultSizeBase(),
  setPreference: (preference) => set({ preference }),
  setSizeBase: (sizeBase) => set({ sizeBase }),
}));

/** A tag the runtime can format with, or `null` when it is not a valid, supported locale. */
function usableLocale(tag: string | undefined): string | null {
  if (tag === undefined || tag.trim() === "") return null;
  try {
    const [canonical] = Intl.getCanonicalLocales(tag);
    if (canonical === undefined) return null;
    return Intl.NumberFormat.supportedLocalesOf([canonical]).length > 0 ? canonical : null;
  } catch {
    return null;
  }
}

/** The operating system's locale as the webview reports it. */
function systemLocale(): string {
  const candidates =
    typeof navigator === "undefined" ? [] : [...(navigator.languages ?? []), navigator.language];
  for (const candidate of candidates) {
    const usable = usableLocale(candidate);
    if (usable !== null) return usable;
  }
  return FALLBACK_LOCALE;
}

/** The locale to format with for a stored preference. An unusable tag falls back to the system's. */
export function resolveLocale(preference: string): string {
  if (preference === LOCALE_SYSTEM) return systemLocale();
  return usableLocale(preference) ?? systemLocale();
}

/** The locale in effect right now. */
export function activeLocale(): string {
  return resolveLocale(useLocaleStore.getState().preference);
}

const numberFormats = new Map<string, Intl.NumberFormat>();

/** A cached `Intl.NumberFormat`; creating one per call is slow in long lists. */
export function numberFormat(
  locale: string,
  options: Intl.NumberFormatOptions = {},
): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let format = numberFormats.get(key);
  if (format === undefined) {
    format = new Intl.NumberFormat(locale, options);
    numberFormats.set(key, format);
  }
  return format;
}

const dateFormats = new Map<string, Intl.DateTimeFormat>();

/** A cached `Intl.DateTimeFormat`. */
export function dateFormat(
  locale: string,
  options: Intl.DateTimeFormatOptions,
): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let format = dateFormats.get(key);
  if (format === undefined) {
    format = new Intl.DateTimeFormat(locale, options);
    dateFormats.set(key, format);
  }
  return format;
}
