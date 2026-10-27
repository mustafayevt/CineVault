import { Channel, isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { ToolCommandError, ToolInstallEvent, ToolStatusDto } from "../../bindings";
import { t } from "../../i18n/catalog";

export type ToolStatus = ToolStatusDto;
export type ToolInstallProgress = ToolInstallEvent;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
class ToolRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ToolRequestError";
    this.code = code;
  }
}

/** Where tool state lives. In the desktop app that is the Rust tools manager. */
export interface ToolsBackend {
  /** False in a plain browser, where there is nothing to download or locate. */
  readonly available: boolean;
  list: () => Promise<ToolStatus[]>;
  /** Sets the path to use for a tool, or clears it with `null`. Returns the updated list. */
  setPath: (tool: string, path: string | null) => Promise<ToolStatus[]>;
  /** Downloads and installs the program that provides `tool`. Returns the updated list. */
  install: (
    tool: string,
    onProgress: (event: ToolInstallProgress) => void,
  ) => Promise<ToolStatus[]>;
  /** Stops the running download. */
  cancel: () => Promise<void>;
  /**
   * Announces downloads that are about to run one after the other, so the work that needs the
   * tools (the first scan) waits for them instead of reporting them missing.
   */
  expect?: (tools: readonly string[]) => Promise<void>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: ToolCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ToolRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: ToolsBackend = {
  available: true,
  list: async () => unwrap(await commands.listTools()),
  setPath: async (tool, path) => unwrap(await commands.setToolPath(tool, path)),
  install: async (tool, onProgress) => {
    const channel = new Channel<ToolInstallEvent>();
    channel.onmessage = onProgress;
    return unwrap(await commands.installTool(tool, channel));
  },
  cancel: async () => {
    await commands.cancelToolInstall();
  },
  expect: async (tools) => {
    await commands.expectTools([...tools]);
  },
};

/** The backend of a plain browser (development): nothing is listed and changes are refused. */
export const unavailableBackend: ToolsBackend = {
  available: false,
  list: () => Promise.resolve([]),
  setPath: () => Promise.reject(new ToolRequestError("unavailable", t("tools.desktopOnly"))),
  install: () => Promise.reject(new ToolRequestError("unavailable", t("tools.desktopOnly"))),
  cancel: () => Promise.resolve(),
};

let override: ToolsBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setToolsBackend(backend: ToolsBackend | null): void {
  override = backend;
}

export function currentToolsBackend(): ToolsBackend {
  if (override !== null) return override;
  return isTauri() ? tauriBackend : unavailableBackend;
}
