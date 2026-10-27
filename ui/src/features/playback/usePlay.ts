import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentPlaybackBackend } from "./playbackBackend";

/**
 * Starts the external player on a movie or an episode, says plainly when it could not (missing
 * file, player not found), and says when the system player could only be given the first part of
 * a stack.
 */
export async function playTitle(id: string, title: string): Promise<void> {
  try {
    const outcome = await currentPlaybackBackend().play(id);
    if (outcome.opened < outcome.parts) {
      showToast({
        message: t("detail.play.firstPartOnly", { title, parts: outcome.parts }),
      });
    }
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    // Said at once where the user is looking, and kept in the notification centre.
    showToast({ message: `${t("detail.play.failed", { title })}. ${reason}` });
    notify({ severity: "error", title: t("detail.play.failed", { title }), detail: reason });
  }
}
