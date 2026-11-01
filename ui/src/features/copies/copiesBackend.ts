import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  ComparisonDto,
  CopiesOfDto,
  CopyMediaDto,
  CopyRowDto,
  LibraryCommandError,
  OrganizeCommandError,
} from "../../bindings";
import { LibraryRequestError } from "../library/libraryBackend";
import { createSyntheticCopies } from "./syntheticCopies";

export type CopiesOf = CopiesOfDto;
export type Comparison = ComparisonDto;
export type CopyRow = CopyRowDto;
export type CopyMedia = CopyMediaDto;

/** The copies of a movie: the same film in the library more than once (spec `movie-copies`). */
export interface CopiesBackend {
  /** The copies of a movie, kept or not; `null` when it has no copy. */
  copiesOf: (id: string) => Promise<CopiesOf | null>;
  /** The Compare copies page of a movie; `null` when it has no copy left. */
  compare: (id: string) => Promise<Comparison | null>;
  /** "Keep both": records that the user keeps every one of `ids`. No file changes. */
  keepAll: (ids: readonly string[]) => Promise<void>;
  /**
   * "Keep this one", checked against the library as it is now: the ids of `others` the delete may
   * take. Rejects with a plain message when the kept copy is gone or offline, one of `others` is
   * no longer a copy in its group, or two copies may be one file. Deletes nothing itself.
   */
  keepCopy: (keep: string, others: readonly string[]) => Promise<string[]>;
  /** "Ask again": the movie's group counts as copies again. Also the Undo of Keep both. */
  askAgain: (id: string) => Promise<void>;
  /** The copy label (`2160p`, `copy 2`) the kept movie could drop from its names, or `null`. */
  labelToDrop: (id: string) => Promise<string | null>;
}

type Result<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

function unwrap<T>(result: Result<T, LibraryCommandError | OrganizeCommandError>): T {
  if (result.status === "error") {
    throw new LibraryRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

export const tauriCopiesBackend: CopiesBackend = {
  copiesOf: async (id) => unwrap(await commands.copiesOf(id)),
  compare: async (id) => unwrap(await commands.compareCopies(id)),
  keepAll: async (ids) => {
    unwrap(await commands.keepAllCopies([...ids]));
  },
  keepCopy: async (keep, others) => unwrap(await commands.keepCopy(keep, [...others])),
  askAgain: async (id) => {
    unwrap(await commands.askAgainCopies(id));
  },
  labelToDrop: async (id) => unwrap(await commands.copyLabelToDrop(id)),
};

let override: CopiesBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setCopiesBackend(backend: CopiesBackend | null): void {
  override = backend;
}

export function currentCopiesBackend(): CopiesBackend {
  if (override !== null) return override;
  return isTauri() ? tauriCopiesBackend : createSyntheticCopies();
}
