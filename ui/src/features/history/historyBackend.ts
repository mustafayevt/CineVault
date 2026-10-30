import { commands, events } from "../../bindings";
import { isServer, transport } from "../../lib/transport";
import type {
  DeleteTitlesDto,
  EraseTitlesDto,
  HistoryCommandError,
  HistoryEntryDto,
  HistoryFilterDto,
  HistoryPageDto,
  HistoryTitleDto,
  UndoReportDto,
} from "../../bindings";
import { createSyntheticHistoryBackend } from "./syntheticHistory";

export type HistoryEntry = HistoryEntryDto;
export type HistoryTitle = HistoryTitleDto;
export type HistoryPage = HistoryPageDto;
export type HistoryFilter = HistoryFilterDto;
export type UndoReport = UndoReportDto;
export type DeleteResult = DeleteTitlesDto;
export type EraseResult = EraseTitlesDto;

/** A refusal from the backend: `message` is ready to show, `code` is stable (see the bindings). */
export class HistoryError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "HistoryError";
    this.code = code;
  }
}

/** Where History, Delete and permanent delete run. */
export interface HistoryBackend {
  /** One page of entries of a tab, newest first. */
  list: (filter: HistoryFilter, offset: number, limit: number) => Promise<HistoryPage>;
  /** Every title of an entry ("Show N more"). */
  titles: (entryId: string) => Promise<HistoryTitle[]>;
  /** The newest entries that touched a title, with that title's line only. */
  forTitle: (id: string) => Promise<HistoryEntry[]>;
  /** The entry that records a rename or move batch, so its toast's Undo points at it. */
  entryForBatch: (batchId: string) => Promise<string | null>;
  /** Bytes sent to the Recycle Bin / Trash that are still there; `null` when unknown. */
  binTotal: () => Promise<number | null>;
  /** Takes a whole entry back. */
  undo: (entryId: string) => Promise<UndoReport>;
  /** Takes back one title's part of an entry. */
  undoTitle: (entryId: string, id: string) => Promise<UndoReport>;
  /** Delete: the files go to the Recycle Bin / Trash and the titles leave the library. */
  deleteTitles: (ids: readonly string[]) => Promise<DeleteResult>;
  /** Delete permanently; `confirm` must be exactly `DELETE` or nothing is erased. */
  eraseTitles: (ids: readonly string[], confirm: string, password?: string) => Promise<EraseResult>;
  /** Calls `handler` when an entry was written or undone. Returns the way to stop listening. */
  onChanged: (handler: () => void) => () => void;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: HistoryCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") throw new HistoryError(result.error.code, result.error.message);
  return result.data;
}

/** The desktop app: the History service and the removal service of the Rust core. */
const tauriHistoryBackend: HistoryBackend = {
  list: async (filter, offset, limit) => unwrap(await commands.historyList(filter, offset, limit)),
  titles: async (entryId) => unwrap(await commands.historyTitles(entryId)),
  forTitle: async (id) => unwrap(await commands.historyForTitle(id)),
  entryForBatch: async (batchId) => unwrap(await commands.historyEntryForBatch(batchId)),
  binTotal: async () => unwrap(await commands.historyBinTotal()),
  undo: async (entryId) => unwrap(await commands.historyUndo(entryId)),
  undoTitle: async (entryId, id) => unwrap(await commands.historyUndoTitle(entryId, id)),
  deleteTitles: async (ids) => unwrap(await commands.deleteTitles([...ids])),
  eraseTitles: async (ids, confirm) => unwrap(await commands.eraseTitles([...ids], confirm)),
  onChanged: (handler) => {
    let stop: (() => void) | null = null;
    let closed = false;
    void events.historyChanged
      .listen(() => handler())
      .then((unlisten) => {
        if (closed) unlisten();
        else stop = unlisten;
      });
    return () => {
      closed = true;
      stop?.();
    };
  },
};

/** The browser server: reads use the shared commands, permanent delete needs the password. */
const browserHistoryBackend: HistoryBackend = {
  ...tauriHistoryBackend,
  eraseTitles: async (ids, confirm, password = "") => {
    const csrf = (window as unknown as Record<string, unknown>).__CINEVAULT_CSRF__;
    const headers = {
      "content-type": "application/json",
      "x-cinevault-csrf": typeof csrf === "string" ? csrf : "",
    };
    const intentResponse = await fetch("/api/delete/intent", {
      method: "POST",
      credentials: "same-origin",
      headers,
      body: JSON.stringify({ ids: [...ids], password }),
    });
    const intent = (await intentResponse.json()) as {
      intent?: string;
      code?: string;
      message?: string;
    };
    if (!intentResponse.ok || intent.intent === undefined) {
      throw new HistoryError(
        intent.code ?? "not_allowed",
        intent.message ?? "Permanent delete is not allowed.",
      );
    }
    const response = await fetch("/api/delete/erase", {
      method: "POST",
      credentials: "same-origin",
      headers,
      body: JSON.stringify({ ids: [...ids], confirm, intent: intent.intent, password }),
    });
    const body = (await response.json()) as EraseResult & { code?: string; message?: string };
    if (!response.ok) {
      throw new HistoryError(
        body.code ?? "not_allowed",
        body.message ?? "The titles could not be erased.",
      );
    }
    return body;
  },
};

let override: HistoryBackend | null = null;
let fallback: HistoryBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setHistoryBackend(backend: HistoryBackend | null): void {
  override = backend;
}

/** The backend in use: the override, the Rust services in the desktop app, else a sample. */
export function currentHistoryBackend(): HistoryBackend {
  if (override !== null) return override;
  if (transport() === "desktop") return tauriHistoryBackend;
  if (isServer()) return browserHistoryBackend;
  fallback ??= createSyntheticHistoryBackend();
  return fallback;
}

/** The key of everything the History page reads. */
export const HISTORY_KEY = ["history"] as const;
