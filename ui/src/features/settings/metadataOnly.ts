import { useSettingsStore } from "./settingsStore";

/**
 * "Metadata only" (Settings › Naming, spec `renamer` "Metadata-only mode"): while it is on,
 * nothing renames or moves a title's files or folders. The backend refuses both; the pages hide
 * Rename and "Move to…".
 */
export const METADATA_ONLY_KEY = "renamer.metadata-only";

/** Whether "Metadata only" is on. Off until it is turned on. */
export function useMetadataOnly(): boolean {
  return useSettingsStore((state) => state.values[METADATA_ONLY_KEY] === true);
}

/** The same, read once outside a component. */
export function isMetadataOnly(): boolean {
  return useSettingsStore.getState().values[METADATA_ONLY_KEY] === true;
}
