import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  DataSourceDto,
  RemapSourceDto,
  RemovedSourceDto,
  SourceCommandError,
  SourceKindDto,
} from "../../bindings";
import { t } from "../../i18n/catalog";
import { createSyntheticSourcesBackend } from "./syntheticSources";

export type SourceKind = SourceKindDto;
export type LibrarySource = DataSourceDto;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class SourceRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "SourceRequestError";
    this.code = code;
  }
}

/** Where library folders are kept. In the desktop app that is the library database. */
export interface SourcesBackend {
  /** False in a plain browser, where there is no library to change. */
  readonly available: boolean;
  list: () => Promise<LibrarySource[]>;
  add: (kind: SourceKind, path: string) => Promise<LibrarySource>;
  remove: (id: number) => Promise<RemovedSourceDto>;
  reorder: (kind: SourceKind, ids: readonly number[]) => Promise<LibrarySource[]>;
  remap: (id: number, path: string) => Promise<RemapSourceDto>;
  /** Gives a folder another label. Nothing on disk changes. */
  rename: (id: number, label: string) => Promise<LibrarySource>;
  /**
   * Moves a folder to the other library type (Settings, Movies | TV shows). No title is deleted;
   * the caller scans the folder again.
   */
  setKind: (id: number, kind: SourceKind) => Promise<LibrarySource>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: SourceCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new SourceRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: SourcesBackend = {
  available: true,
  list: async () => unwrap(await commands.listDataSources()),
  add: async (kind, path) => unwrap(await commands.addDataSource(kind, path)),
  remove: async (id) => unwrap(await commands.removeDataSource(id)),
  reorder: async (kind, ids) => unwrap(await commands.reorderDataSources(kind, [...ids])),
  remap: async (id, path) => unwrap(await commands.remapDataSource(id, path)),
  rename: async (id, label) => unwrap(await commands.renameDataSourceLabel(id, label)),
  setKind: async (id, kind) => unwrap(await commands.changeDataSourceKind(id, kind)),
};

/** The backend of a plain browser (development): nothing to list, and changes are refused. */
const unavailableBackend: SourcesBackend = {
  available: false,
  list: () => Promise.resolve([]),
  add: () => Promise.reject(new SourceRequestError("unavailable", t("sources.desktopOnly"))),
  remove: () => Promise.reject(new SourceRequestError("unavailable", t("sources.desktopOnly"))),
  reorder: () => Promise.reject(new SourceRequestError("unavailable", t("sources.desktopOnly"))),
  remap: () => Promise.reject(new SourceRequestError("unavailable", t("sources.desktopOnly"))),
  rename: () => Promise.reject(new SourceRequestError("unavailable", t("sources.desktopOnly"))),
  setKind: () => Promise.reject(new SourceRequestError("unavailable", t("sources.desktopOnly"))),
};

let override: SourcesBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setSourcesBackend(backend: SourcesBackend | null): void {
  override = backend;
}

let synthetic: SourcesBackend | null = null;

export function currentSourcesBackend(): SourcesBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriBackend;
  // `?first-run` previews the setup in a plain browser against an in-memory list of folders;
  // `?synthetic-folders` starts that list with the folders the sidebar shows (Settings check).
  const params = new URLSearchParams(window.location.search);
  if (params.has("first-run") || params.has("synthetic-folders")) {
    synthetic ??= createSyntheticSourcesBackend(params.has("synthetic-folders"));
    return synthetic;
  }
  return unavailableBackend;
}
