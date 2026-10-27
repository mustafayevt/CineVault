import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { EditCommandError } from "../../bindings";
import { createSyntheticCurationBackend } from "./syntheticCuration";

/** A refusal from the backend: `message` is ready to show. */
class CurationError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "CurationError";
    this.code = code;
  }
}

/** What clearing NEW everywhere changed. */
export interface ClearedNew {
  changed: readonly string[];
  titles: number;
}

/** The kinds the NEW flag can be cleared for. */
export type NewFlagKind = "movie" | "show" | "episode";

/** Where the NEW flag reads and writes: the library, or a sample in a browser. */
export interface CurationBackend {
  /** False in a plain browser, where nothing reaches a library. */
  readonly available: boolean;
  /**
   * Clears the NEW flag of the titles; a show's NEW episodes lose it too. Resolves to the ids
   * that were flagged, for Undo.
   */
  clearNew: (ids: readonly string[]) => Promise<readonly string[]>;
  /**
   * Clears the NEW flag of a title whose page was opened: its own flag only, so an opened show
   * keeps its NEW episodes. Resolves to the ids that changed.
   */
  clearNewOnOpen: (id: string) => Promise<readonly string[]>;
  /**
   * Clears the NEW flag of every title, or of one kind. Resolves to the items that changed (for
   * Undo; episodes included) and how many titles they make up (an episode counts as its show).
   */
  clearAllNew: (kind?: NewFlagKind) => Promise<ClearedNew>;
  /** Flags the titles NEW again (Undo of a clear). */
  restoreNew: (ids: readonly string[]) => Promise<void>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: EditCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new CurationError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriCurationBackend: CurationBackend = {
  available: true,
  clearNew: async (ids) => unwrap(await commands.clearNewFlags([...ids])).changed,
  clearNewOnOpen: async (id) => unwrap(await commands.clearNewOnOpen(id)).changed,
  clearAllNew: async (kind) => {
    const report = unwrap(await commands.clearAllNewFlags(kind ?? null));
    return { changed: report.changed, titles: report.titles };
  },
  restoreNew: async (ids) => {
    unwrap(await commands.restoreNewFlags([...ids]));
  },
};

let override: CurationBackend | null = null;
let fallback: CurationBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setCurationBackend(backend: CurationBackend | null): void {
  override = backend;
}

export function currentCurationBackend(): CurationBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriCurationBackend;
  fallback ??= createSyntheticCurationBackend();
  return fallback;
}
