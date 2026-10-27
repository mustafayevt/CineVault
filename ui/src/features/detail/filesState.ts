import { t } from "../../i18n/catalog";

/** What a title's files are doing, from the availability the detail page reports. */
export type FilesState = "missing" | "offline";

/** `missing` or `offline` when the files cannot be reached; `null` for a title that is fine. */
export function filesStateOf(availability: string): FilesState | null {
  return availability === "missing" || availability === "offline" ? availability : null;
}

/** Why Play does nothing, for a title whose files are not there. */
export function playBlockedFor(
  state: FilesState | null,
  kind: "movie" | "show" | "episode" = "movie",
): string | undefined {
  if (state === null) return undefined;
  if (state === "offline") return t("detail.files.playOffline");
  return t(kind === "movie" ? "detail.files.playMissing" : "detail.files.playMissingTv");
}
