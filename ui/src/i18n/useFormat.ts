import { useMemo } from "react";

import { createFormatters } from "./format";
import type { Formatters } from "./format";
import { resolveLocale, useLocaleStore } from "./locale";

/**
 * The date, number and file-size formatters for the selected locale and size unit. A component
 * that shows any of them calls this hook, so it redraws when the user changes the regional format
 * or switches between base 10 (GB) and base 2 (GiB) in Settings.
 */
export function useFormat(): Formatters {
  const preference = useLocaleStore((state) => state.preference);
  const sizeBase = useLocaleStore((state) => state.sizeBase);
  return useMemo(
    () => createFormatters(resolveLocale(preference), sizeBase),
    [preference, sizeBase],
  );
}

/** The formatters in effect now, for code outside React (stores, command handlers). */
export function currentFormat(): Formatters {
  const { preference, sizeBase } = useLocaleStore.getState();
  return createFormatters(resolveLocale(preference), sizeBase);
}
