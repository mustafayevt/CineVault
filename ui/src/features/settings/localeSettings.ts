import { useEffect } from "react";

import { useLocaleStore } from "../../i18n/locale";
import type { SizeBase } from "../../i18n/locale";
import { useSetting } from "./settingsStore";

/**
 * Applies the regional format (`general.locale`) and the file size units (`advanced.size-units`)
 * as they change, so every date, number and size in the app follows them at once.
 */
export function useLocaleSettings(): void {
  const locale = useSetting<string>("general.locale");
  const sizeUnits = useSetting<SizeBase>("advanced.size-units");
  const setPreference = useLocaleStore((state) => state.setPreference);
  const setSizeBase = useLocaleStore((state) => state.setSizeBase);

  useEffect(() => {
    setPreference(locale);
  }, [locale, setPreference]);

  useEffect(() => {
    setSizeBase(sizeUnits);
  }, [sizeUnits, setSizeBase]);
}
