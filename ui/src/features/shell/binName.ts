import { t } from "../../i18n/catalog";
import type { Platform } from "./platform";

/**
 * The name of the bin that deleted files go to: the Recycle Bin on Windows, the Trash on macOS and
 * on Linux (the freedesktop Trash). Messages take it as their `{bin}` parameter.
 */
export function binName(platform: Platform): string {
  return platform === "windows" ? t("common.bin.recycle") : t("common.bin.trash");
}
