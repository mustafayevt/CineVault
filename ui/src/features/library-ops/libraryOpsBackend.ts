import { Channel, isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  DeleteReportDto,
  LibraryOpsCommandError,
  MoveAppliedDto,
  MoveControlDto,
  MovePlanDto,
  MoveProgressDto,
  MoveTitlePreviewDto,
  RemovalPlanDto,
  RemoveReportDto,
  UndoOutcomeDto,
} from "../../bindings";
import type { LibrarySource } from "../library-sources/sourcesBackend";
import { createSyntheticLibraryOpsBackend } from "./syntheticLibraryOps";

export type MovePlan = MovePlanDto;
export type MoveApplied = MoveAppliedDto;
export type MoveProgress = MoveProgressDto;
/** Everything that moves with one title: every file and folder, and the verdict on the move. */
export type MoveTitlePreview = MoveTitlePreviewDto;
export type MoveControl = MoveControlDto;
export type RemovalPlan = RemovalPlanDto;
export type RemoveReport = RemoveReportDto;
export type DeleteReport = DeleteReportDto;
export type UndoOutcome = UndoOutcomeDto;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class LibraryOpsError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "LibraryOpsError";
    this.code = code;
  }
}

/** Where the move, remove and delete operations run. */
export interface LibraryOpsBackend {
  /** False in a plain browser, where there is no library to change. */
  readonly available: boolean;
  /** Says what moving the titles to a data source would do. Changes nothing. */
  planMove: (ids: readonly string[], target: number) => Promise<MovePlan>;
  /** The library folders a title can move to: every data source, with its label and kind. */
  listSources: () => Promise<LibrarySource[]>;
  /**
   * Lists every file and folder that would move with one title, with the destination, the size,
   * the free space and the conflicts. Changes nothing.
   */
  previewTitle: (id: string, target: number) => Promise<MoveTitlePreview>;
  /** Pauses, resumes or cancels the move that is running. */
  controlMove: (action: MoveControl) => Promise<void>;
  /** Moves the titles as one journal batch; `onProgress` follows each title and file. */
  applyMove: (
    ids: readonly string[],
    target: number,
    onProgress: (progress: MoveProgress) => void,
  ) => Promise<MoveApplied>;
  /** Takes a journal batch back (the same undo Organize uses). */
  undoBatch: (batchId: string) => Promise<UndoOutcome>;
  /** Says what removing or deleting the titles would touch. Changes nothing. */
  planRemoval: (ids: readonly string[]) => Promise<RemovalPlan>;
  /** Takes the titles out of the library; their files stay. */
  removeFromLibrary: (ids: readonly string[], ignoreFolders: boolean) => Promise<RemoveReport>;
  /** Sends the files to the system trash, then removes the titles. */
  deleteWithFiles: (ids: readonly string[]) => Promise<DeleteReport>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: LibraryOpsCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new LibraryOpsError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriLibraryOpsBackend: LibraryOpsBackend = {
  available: true,
  planMove: async (ids, target) => unwrap(await commands.planChangeSource([...ids], target)),
  listSources: async () => {
    const result = await commands.listDataSources();
    if (result.status === "error")
      throw new LibraryOpsError(result.error.code, result.error.message);
    return result.data;
  },
  previewTitle: async (id, target) => unwrap(await commands.previewChangeSourceTitle(id, target)),
  controlMove: async (action) => {
    await commands.controlMove(action);
  },
  applyMove: async (ids, target, onProgress) => {
    const channel = new Channel<MoveProgressDto>();
    channel.onmessage = onProgress;
    return unwrap(await commands.applyChangeSource([...ids], target, channel));
  },
  undoBatch: async (batchId) => {
    const result = await commands.organizeUndo(batchId);
    if (result.status === "error") {
      throw new LibraryOpsError(result.error.code, result.error.message);
    }
    return result.data;
  },
  planRemoval: async (ids) => unwrap(await commands.planRemoval([...ids])),
  removeFromLibrary: async (ids, ignoreFolders) =>
    unwrap(await commands.removeFromLibrary([...ids], ignoreFolders)),
  deleteWithFiles: async (ids) => unwrap(await commands.deleteWithFiles([...ids])),
};

let override: LibraryOpsBackend | null = null;
let fallback: LibraryOpsBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setLibraryOpsBackend(backend: LibraryOpsBackend | null): void {
  override = backend;
}

/** The backend in use: the override, the Rust service in the desktop app, else a sample. */
export function currentLibraryOpsBackend(): LibraryOpsBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriLibraryOpsBackend;
  fallback ??= createSyntheticLibraryOpsBackend();
  return fallback;
}
