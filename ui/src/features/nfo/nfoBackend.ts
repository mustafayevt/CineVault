import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { NfoActionDto, NfoCommandError, NfoScopeDto, NfoUndoDto } from "../../bindings";
import { t } from "../../i18n/catalog";

export type NfoScope = NfoScopeDto;
export type NfoActionResult = NfoActionDto;
export type NfoUndoResult = NfoUndoDto;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class NfoRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "NfoRequestError";
    this.code = code;
  }
}

/** Where NFO files are read and written. In the desktop app that is the Rust NFO service. */
export interface NfoBackend {
  /** False in a plain browser, where there are no files. */
  readonly available: boolean;
  /** Loads the values from the NFO files into the library. */
  read: (scope: NfoScope) => Promise<NfoActionResult>;
  /** Writes the NFO files from the library. */
  rewrite: (scope: NfoScope) => Promise<NfoActionResult>;
  /** Reverses one action by its token. */
  undo: (token: string) => Promise<NfoUndoResult>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: NfoCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new NfoRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: NfoBackend = {
  available: true,
  read: async (scope) => unwrap(await commands.readNfo(scope)),
  rewrite: async (scope) => unwrap(await commands.rewriteNfo(scope)),
  undo: async (token) => unwrap(await commands.undoNfoAction(token)),
};

function unavailable(): Promise<never> {
  return Promise.reject(new NfoRequestError("unavailable", t("nfo.desktopOnly")));
}

/** The backend of a plain browser (development): there are no files to read or write. */
const unavailableBackend: NfoBackend = {
  available: false,
  read: unavailable,
  rewrite: unavailable,
  undo: unavailable,
};

let override: NfoBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setNfoBackend(backend: NfoBackend | null): void {
  override = backend;
}

export function currentNfoBackend(): NfoBackend {
  if (override !== null) return override;
  return isTauri() ? tauriBackend : unavailableBackend;
}
