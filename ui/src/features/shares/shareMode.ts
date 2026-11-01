import { t } from "../../i18n/catalog";
import type { SourceKind } from "../library-sources/sourcesBackend";
import type { ShareMode } from "./NetworkShareForm";

/** The title of the page: a new share, or the share that is offline. */
export function shareTitle(mode: ShareMode): string {
  return mode.kind === "connect"
    ? t("networkShare.title")
    : t("networkShare.titleAgain", { address: mode.share.address });
}

/** The line under the title: where the folder goes, or what an offline share holds. */
export function shareSubtitle(mode: ShareMode, folders: number): string {
  if (mode.kind === "connect") return t(`networkShare.sub.${mode.library ?? "any"}`);
  return t("networkShare.subAgain", {
    count: folders,
    reason: t(`networkShare.reason.${mode.share.reason ?? "not_connected"}`),
  });
}

/** `?kind=movie|tv` and `?share=<id>` of the route. */
export interface NetworkShareSearch {
  kind?: SourceKind;
  share?: number;
}

/** Reads the search of `/network-share`; anything damaged is dropped. */
export function validateNetworkShareSearch(search: Record<string, unknown>): NetworkShareSearch {
  const share = Number(search.share);
  return {
    ...(search.kind === "movie" || search.kind === "tv" ? { kind: search.kind } : {}),
    ...(Number.isInteger(share) && share > 0 ? { share } : {}),
  };
}
