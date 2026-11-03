import { useQuery } from "@tanstack/react-query";
import { Channel, isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  MoveProgressDto,
  ResumeOutcomeDto,
  RollBackOutcomeDto,
  UnfinishedBatchDto,
} from "../../bindings";
import { LibraryOpsError } from "../library-ops/libraryOpsBackend";
import type { MoveProgress } from "../library-ops/libraryOpsBackend";

export type UnfinishedBatch = UnfinishedBatchDto;
export type ResumeOutcome = ResumeOutcomeDto;
export type RollBackOutcome = RollBackOutcomeDto;

/** Where unfinished moves and renames are listed, resumed and rolled back. */
export interface RecoveryBackend {
  /** The moves and renames that did not finish, newest first. A running one is not listed. */
  list: () => Promise<UnfinishedBatch[]>;
  /** Carries the batch on; `onProgress` follows it as for a move. Pause and Cancel as for a move. */
  resume: (batchId: string, onProgress: (progress: MoveProgress) => void) => Promise<ResumeOutcome>;
  /** Takes back what the batch made; the originals were never touched. */
  rollBack: (batchId: string) => Promise<RollBackOutcome>;
}

type Result<T> =
  { status: "ok"; data: T } | { status: "error"; error: { code: string; message: string } };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new LibraryOpsError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriRecoveryBackend: RecoveryBackend = {
  list: async () => unwrap(await commands.listUnfinishedBatches()),
  resume: async (batchId, onProgress) => {
    const channel = new Channel<MoveProgressDto>();
    channel.onmessage = onProgress;
    return unwrap(await commands.resumeUnfinishedBatch(batchId, channel));
  },
  rollBack: async (batchId) => unwrap(await commands.rollBackUnfinishedBatch(batchId)),
};

/** A plain browser has no journal: nothing is unfinished there. */
const emptyRecoveryBackend: RecoveryBackend = {
  list: () => Promise.resolve([]),
  resume: (batchId) => Promise.reject(new LibraryOpsError("unknown_batch", batchId)),
  rollBack: (batchId) => Promise.reject(new LibraryOpsError("unknown_batch", batchId)),
};

let override: RecoveryBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setRecoveryBackend(backend: RecoveryBackend | null): void {
  override = backend;
}

/** The backend in use: the override, the Rust service in the desktop app, else an empty one. */
export function currentRecoveryBackend(): RecoveryBackend {
  if (override !== null) return override;
  return isTauri() ? tauriRecoveryBackend : emptyRecoveryBackend;
}

export const UNFINISHED_KEY = ["unfinished-batches"] as const;

/** The unfinished moves and renames, read again after every move, resume and roll back. */
export function useUnfinishedBatches() {
  return useQuery({
    queryKey: UNFINISHED_KEY,
    queryFn: () => currentRecoveryBackend().list(),
    staleTime: 0,
  });
}
