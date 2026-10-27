import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  ItemLifecycleDto,
  LibraryCommandError,
  LifecycleCountsDto,
  LifecycleDto,
} from "../../bindings";
import { syntheticCountFromLocation } from "../library/libraryBackend";
import { syntheticCard, syntheticPipelineCounts } from "../library/syntheticLibrary";

export type LifecycleCounts = LifecycleCountsDto;
export type Lifecycle = LifecycleDto;
export type ItemLifecycle = ItemLifecycleDto;

/** The library types that have a pipeline. */
export type PipelineKind = "movies" | "shows";

/** A refusal from the backend: `message` is ready to show. */
class LifecycleRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "LifecycleRequestError";
    this.code = code;
  }
}

/** Where lifecycles come from: the database in the desktop app, fixtures in a plain browser. */
export interface LifecycleBackend {
  /**
   * The pipeline counts of a library type. The sidebar, Home, Needs review, Organize, the filter
   * chips and Activity all read this one answer, so they cannot disagree.
   */
  counts: (kind: PipelineKind) => Promise<LifecycleCounts>;
  /** The stage, flags and explanation lines of titles; a title without a pipeline is left out. */
  get: (ids: readonly string[]) => Promise<readonly ItemLifecycle[]>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: LibraryCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new LifecycleRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriLifecycleBackend: LifecycleBackend = {
  counts: async (kind) => unwrap(await commands.lifecycleCounts(kind)),
  get: async (ids) => unwrap(await commands.getLifecycles([...ids])),
};

/** An in-memory backend over the same fixture library the library screens show. */
export function createSyntheticLifecycleBackend(count: number): LifecycleBackend {
  return {
    counts: (kind) => {
      const show = kind === "shows";
      const total = show ? Math.ceil(count / 8) : count;
      return Promise.resolve(syntheticPipelineCounts(total, show));
    },
    get: (ids) =>
      Promise.resolve(
        ids.flatMap((id) => {
          const index = Number.parseInt(id, 10) - 1;
          const lifecycle =
            Number.isFinite(index) && index >= 0 ? syntheticCard(index).lifecycle : null;
          return lifecycle === null ? [] : [{ id, lifecycle }];
        }),
      ),
  };
}

let override: LifecycleBackend | null = null;
let fallback: LifecycleBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setLifecycleBackend(backend: LifecycleBackend | null): void {
  override = backend;
}

export function currentLifecycleBackend(): LifecycleBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriLifecycleBackend;
  fallback ??= createSyntheticLifecycleBackend(syntheticCountFromLocation(window.location.search));
  return fallback;
}
