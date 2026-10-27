import { useCallback } from "react";

import { t } from "../../i18n/catalog";
import { currentOpenFolderBackend } from "./openFolderBackend";
import { notify } from "../notifications/notificationStore";

/** A folder and a name, joined with the separator the folder uses. */
export function joinPath(folder: string, name: string): string {
  if (folder === "") return name;
  const separator = folder.includes("\\") && !folder.includes("/") ? "\\" : "/";
  return folder.endsWith(separator) ? `${folder}${name}` : `${folder}${separator}${name}`;
}

/**
 * Opens a folder in the system file manager. Only folders open (the backend refuses a program or
 * a file it does not know); a failure goes to Activity and names the title.
 */
export function useOpenFolder(title: string): (folder: string) => void {
  return useCallback(
    (folder: string) => {
      currentOpenFolderBackend()
        .open(folder)
        .catch((error: unknown) =>
          notify({
            severity: "error",
            title: t("detail.more.openFailed", { title }),
            detail: error instanceof Error ? error.message : String(error),
          }),
        );
    },
    [title],
  );
}
