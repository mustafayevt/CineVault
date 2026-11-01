import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  ConnectShareInput,
  NetworkShareDto,
  ReconnectShareInput,
  ShareAddedDto,
  ShareCommandError,
  ShareSupportDto,
} from "../../bindings";
import { createSyntheticSharesBackend } from "./syntheticShares";

export type ShareSupport = ShareSupportDto;
export type NetworkShare = NetworkShareDto;
export type ShareAdded = ShareAddedDto;
export type ConnectShare = ConnectShareInput;
export type ReconnectShare = ReconnectShareInput;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class ShareRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ShareRequestError";
    this.code = code;
  }
}

/** Where network shares are connected. In the desktop app that is the system's own tools. */
export interface SharesBackend {
  /** What this computer can do with shares: connect them at all, and where they appear. */
  support: () => Promise<ShareSupport>;
  /** Connects the share and adds the folder inside it as a library folder. */
  connect: (input: ConnectShare) => Promise<ShareAdded>;
  /** Connects a known share again with a new login. */
  connectAgain: (input: ReconnectShare) => Promise<NetworkShare>;
  /** Stops the connect that is running. */
  cancel: () => Promise<boolean>;
  /** Every share CineVault connected. */
  list: () => Promise<NetworkShare[]>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: ShareCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new ShareRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: SharesBackend = {
  support: async () => unwrap(await commands.networkShareSupport()),
  connect: async (input) => unwrap(await commands.connectNetworkShare(input)),
  connectAgain: async (input) => unwrap(await commands.connectNetworkShareAgain(input)),
  cancel: () => commands.cancelNetworkShareConnect(),
  list: async () => unwrap(await commands.listNetworkShares()),
};

let override: SharesBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setSharesBackend(backend: SharesBackend | null): void {
  override = backend;
}

let synthetic: SharesBackend | null = null;

export function currentSharesBackend(): SharesBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriBackend;
  // A plain browser (development and the screenshot check) answers from memory.
  synthetic ??= createSyntheticSharesBackend(new URLSearchParams(window.location.search));
  return synthetic;
}
