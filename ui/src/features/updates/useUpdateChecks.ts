import { useEffect } from "react";

import { useSettingsStore } from "../settings/settingsStore";
import { useUpdateStore } from "./updateStore";

/** The first background check waits until the library has had the window to itself. */
export const FIRST_CHECK_DELAY_MS = 15_000;
/** Afterwards the backend is asked every hour; it only contacts the channel when the interval has passed. */
const RECHECK_EVERY_MS = 60 * 60 * 1000;

/**
 * Runs the background update check once the settings are loaded. Whether anything is requested at
 * all is decided in the backend: updates are off until a channel is configured.
 */
export function useUpdateChecks(): void {
  const ready = useSettingsStore((state) => state.status === "ready");
  useEffect(() => {
    if (!ready) return;
    const check = () => void useUpdateStore.getState().checkInBackground();
    const first = window.setTimeout(check, FIRST_CHECK_DELAY_MS);
    const repeat = window.setInterval(check, RECHECK_EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(repeat);
    };
  }, [ready]);
}
