import { useSettingsStore } from "../settings/settingsStore";

export const NOTICE_KEY = "torrents.notice-accepted";

/** Whether the responsibility notice was accepted; `null` while the settings are loading. */
export function useTorrentNotice(): boolean | null {
  const ready = useSettingsStore((state) => state.status === "ready");
  const accepted = useSettingsStore((state) => state.values[NOTICE_KEY] === true);
  return ready ? accepted : null;
}
