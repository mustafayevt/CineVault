import { useEffect, useRef } from "react";

import { useSettingsStore } from "../settings/settingsStore";
import { useAppVersion } from "../updates/useAppVersion";
import { startWhatsNew } from "./whatsNew";

/**
 * Once per session, when the settings are loaded and the running version is known, shows the
 * "What's new" card if the app was updated since it last ran (see `decideWhatsNew`).
 */
export function useWhatsNew(): void {
  const ready = useSettingsStore((state) => state.status === "ready");
  const version = useAppVersion();
  const started = useRef(false);
  useEffect(() => {
    if (!ready || version === "" || started.current) return;
    started.current = true;
    startWhatsNew(version);
  }, [ready, version]);
}
