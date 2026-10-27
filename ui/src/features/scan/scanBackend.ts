import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import { SYNTHETIC_SOURCE_HEALTH } from "./syntheticSources";
import type {
  MediaInfoStatusDto,
  MissingInSourceDto,
  RescanStartedDto,
  ScanCommandError,
  SourceHealthDto,
} from "../../bindings";

export type RescanStarted = RescanStartedDto;
export type MissingInSource = MissingInSourceDto;
export type SourceHealth = SourceHealthDto;
export type MediaInfoStatus = MediaInfoStatusDto;

/** A refusal from the backend: `message` is ready to show. */
export class ScanRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ScanRequestError";
    this.code = code;
  }
}

/** Where scanning happens. In the desktop app that is the backend's scan pipeline. */
export interface ScanBackend {
  /** False in a plain browser, where there is nothing to scan. */
  readonly available: boolean;
  /** Queues a rescan of every library folder. */
  rescan: () => Promise<RescanStarted>;
  /** The videos of every library folder that belong to no title. */
  findMissing: () => Promise<MissingInSource[]>;
  /** Every library folder with whether it is reachable and what it holds. */
  sourceHealth: () => Promise<SourceHealth[]>;
  /** Reads the media info of titles that are still waiting for it. */
  readPendingMediaInfo: () => Promise<MediaInfoStatus>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: ScanCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ScanRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriScanBackend: ScanBackend = {
  available: true,
  rescan: async () => unwrap(await commands.rescanLibrary()),
  findMissing: async () => unwrap(await commands.findMissingFiles()),
  sourceHealth: async () => unwrap(await commands.listSourceHealth()),
  readPendingMediaInfo: async () => unwrap(await commands.readPendingMediaInfo()),
};

/** The backend of a plain browser (development): nothing is scanned and no folder is listed. */
export const unavailableScanBackend: ScanBackend = {
  available: false,
  rescan: () => Promise.resolve({ job_id: 0, sources: 0, media_info: "ready" }),
  findMissing: () => Promise.resolve([]),
  sourceHealth: () => Promise.resolve([...SYNTHETIC_SOURCE_HEALTH]),
  readPendingMediaInfo: () => Promise.resolve("ready"),
};

let override: ScanBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setScanBackend(backend: ScanBackend | null): void {
  override = backend;
}

export function currentScanBackend(): ScanBackend {
  if (override !== null) return override;
  return isTauri() ? tauriScanBackend : unavailableScanBackend;
}
