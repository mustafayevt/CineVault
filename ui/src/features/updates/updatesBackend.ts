import { Channel, isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  UpdateCheckDto,
  UpdateCommandError,
  UpdateInstallDto,
  UpdateOfferDto,
  UpdateProgressEvent,
  UpdateStatusDto,
} from "../../bindings";
import { t } from "../../i18n/catalog";

export type UpdateOffer = UpdateOfferDto;
export type UpdateCheck = UpdateCheckDto;
export type UpdateStatus = UpdateStatusDto;
export type UpdateProgress = UpdateProgressEvent;
export type UpdateInstall = UpdateInstallDto;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class UpdateRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "UpdateRequestError";
    this.code = code;
  }
}

/** Where update state lives. In the desktop app that is the Rust update service. */
export interface UpdatesBackend {
  /** False in a plain browser, where there is nothing to check or install. */
  readonly available: boolean;
  status: () => Promise<UpdateStatus>;
  /** `manual` asks the channel now; otherwise the check only runs when it is due. */
  check: (manual: boolean) => Promise<UpdateCheck>;
  /**
   * Downloads and verifies the installer of the offered `version`, then installs it: on a copy the
   * Windows setup installed the setup runs by itself and the app restarts (`restarting`);
   * otherwise the installer opens and `reason` says why. Without `stopJobs` a running job refuses
   * with the code `jobs_running`.
   */
  install: (
    version: string,
    stopJobs: boolean,
    onProgress: (event: UpdateProgress) => void,
  ) => Promise<UpdateInstall>;
  /** Stops the running download. */
  cancel: () => Promise<void>;
  /** Jobs that are queued or running, which an update waits for. */
  blockingJobs: () => Promise<number>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: UpdateCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new UpdateRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: UpdatesBackend = {
  available: true,
  status: () => commands.getUpdateStatus(),
  check: async (manual) => unwrap(await commands.checkForUpdate(manual)),
  install: async (version, stopJobs, onProgress) => {
    const channel = new Channel<UpdateProgressEvent>();
    channel.onmessage = onProgress;
    return unwrap(await commands.installUpdate(version, stopJobs, channel));
  },
  cancel: async () => {
    await commands.cancelUpdateDownload();
  },
  blockingJobs: () => commands.updateBlockingJobs(),
};

/** The backend of a plain browser (development): there is never an update. */
const unavailableBackend: UpdatesBackend = {
  available: false,
  status: () =>
    Promise.resolve({ current_version: "", channel_set: false, enabled: false, offer: null }),
  check: () => Promise.resolve({ outcome: "disabled", offer: null }),
  install: () => Promise.reject(new UpdateRequestError("unavailable", t("updates.desktopOnly"))),
  cancel: () => Promise.resolve(),
  blockingJobs: () => Promise.resolve(0),
};

let override: UpdatesBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setUpdatesBackend(backend: UpdatesBackend | null): void {
  override = backend;
}

export function currentUpdatesBackend(): UpdatesBackend {
  if (override !== null) return override;
  return isTauri() ? tauriBackend : unavailableBackend;
}
