import { useSetting } from "../settings/settingsStore";

/**
 * The title to show for an item: its original-language title when `providers.prefer-original-title`
 * is on and the item has one, else the title in the language of the library.
 */
export function displayTitle(
  title: string,
  originalTitle: string | null | undefined,
  preferOriginal: boolean,
): string {
  if (!preferOriginal) return title;
  const original = originalTitle?.trim() ?? "";
  return original === "" ? title : original;
}

/** Whether titles show in their original language (`providers.prefer-original-title`), live. */
export function usePreferOriginalTitle(): boolean {
  return useSetting<boolean>("providers.prefer-original-title");
}
