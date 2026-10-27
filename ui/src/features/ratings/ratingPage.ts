import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import { t } from "../../i18n/catalog";
import { showToast } from "../toasts/toastStore";

/** Hands a web address to the browser. */
export type RatingPageOpener = (url: string) => Promise<void>;

/** The desktop app: the Rust opener, which takes `http` and `https` addresses only. */
const tauriOpener: RatingPageOpener = async (url) => {
  const answer = await commands.openRatingPage(url);
  if (answer.status === "error") throw new Error(answer.error.message);
};

/** A plain browser (development, screenshots): a new tab. */
const browserOpener: RatingPageOpener = (url) => {
  window.open(url, "_blank", "noopener,noreferrer");
  return Promise.resolve();
};

const override: RatingPageOpener | null = null;

function currentOpener(): RatingPageOpener {
  if (override !== null) return override;
  return isTauri() ? tauriOpener : browserOpener;
}

/** Opens the title's page on a rating source's site; says so when it cannot. */
export async function openRatingPage(url: string, source: string): Promise<void> {
  try {
    await currentOpener()(url);
  } catch {
    showToast({ message: t("ratings.link.failed", { source }), tone: "error" });
  }
}
