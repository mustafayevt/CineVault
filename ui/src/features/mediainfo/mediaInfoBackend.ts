import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { ItemActionResultDto } from "../../bindings";
import { t } from "../../i18n/catalog";

export type ReloadResult = ItemActionResultDto;

/** A refusal from the backend: `message` is ready to show, `code` is stable. */
export class MediaInfoRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "MediaInfoRequestError";
    this.code = code;
  }
}

/** Where media info is read again. In the desktop app that is the batch runner. */
export interface MediaInfoBackend {
  /**
   * Forgets what is stored about the videos of these titles and reads them from the files again.
   * Resolves when the job has ended; the job shows in the sidebar progress card while it runs.
   */
  reload: (ids: readonly string[]) => Promise<ReloadResult>;
}

const tauriBackend: MediaInfoBackend = {
  reload: async (ids) => {
    const result = await commands.runItemAction([...ids], "reload-media-info");
    if (result.status === "error") {
      throw new MediaInfoRequestError(result.error.code, result.error.message);
    }
    return result.data;
  },
};

/** A plain browser has no files to read. */
const unavailableBackend: MediaInfoBackend = {
  reload: () =>
    Promise.reject(new MediaInfoRequestError("unavailable", t("library.reloadMedia.desktopOnly"))),
};

let override: MediaInfoBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setMediaInfoBackend(backend: MediaInfoBackend | null): void {
  override = backend;
}

export function currentMediaInfoBackend(): MediaInfoBackend {
  if (override !== null) return override;
  return isTauri() ? tauriBackend : unavailableBackend;
}
