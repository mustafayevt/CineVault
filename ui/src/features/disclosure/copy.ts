import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";

/**
 * Copies text to the clipboard and says so with a toast; a failure goes to Activity, never a
 * dialog. `message` is the confirmation ("Path copied").
 */
export async function copyText(text: string, message: string, failure: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    showToast({ message });
    return true;
  } catch {
    notify({ severity: "error", title: failure, detail: "" });
    return false;
  }
}
