import { t } from "../../i18n/catalog";
import { copyText } from "../disclosure/copy";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { TorrentRequestError, currentTorrentBackend } from "./torrentBackend";
import type { TorrentRelease } from "./torrentBackend";

/** Copies the magnet link of a release; says so, or says why it could not. */
export async function copyMagnet(release: Pick<TorrentRelease, "magnet">): Promise<boolean> {
  if (release.magnet === null) return false;
  return copyText(release.magnet, t("torrents.toast.copied"), t("torrents.toast.copyFailed"));
}

function code(error: unknown): string {
  return error instanceof TorrentRequestError ? error.code : "failed";
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Hands a release to the person's torrent app: the magnet link, or the `.torrent` file when the
 * release has no magnet. A confirmation toast says it was sent; when no program is set up for
 * magnet links the toast says so and offers to copy the link instead.
 */
export async function openRelease(release: TorrentRelease): Promise<boolean> {
  if (release.magnet === null) return downloadTorrent(release);
  const magnet = release.magnet;
  try {
    await currentTorrentBackend().openMagnet(magnet);
    showToast({ message: t("torrents.toast.sent") });
    return true;
  } catch (error) {
    switch (code(error)) {
      case "no_handler":
        showToast({
          message: t("torrents.toast.noHandler"),
          tone: "error",
          action: {
            label: t("torrents.action.copyMagnet"),
            onClick: () => void copyMagnet(release),
          },
        });
        break;
      case "invalid_magnet":
        showToast({ message: t("torrents.toast.invalidMagnet"), tone: "error" });
        break;
      case "notice_not_accepted":
        showToast({ message: t("torrents.toast.noticeFirst"), tone: "error" });
        break;
      default:
        notify({
          severity: "error",
          title: t("torrents.toast.openFailed"),
          detail: messageOf(error),
        });
    }
    return false;
  }
}

/** Downloads the `.torrent` file of a release and opens it in the person's torrent app. */
async function downloadTorrent(release: TorrentRelease): Promise<boolean> {
  if (release.torrentUrl === null) return false;
  try {
    await currentTorrentBackend().openTorrentFile(release.torrentUrl, release.name);
    showToast({ message: t("torrents.toast.sent") });
    return true;
  } catch (error) {
    if (code(error) === "no_handler") {
      showToast({ message: t("torrents.toast.noFileHandler"), tone: "error" });
    } else {
      notify({
        severity: "error",
        title: t("torrents.toast.downloadFailed"),
        detail: messageOf(error),
      });
    }
    return false;
  }
}
