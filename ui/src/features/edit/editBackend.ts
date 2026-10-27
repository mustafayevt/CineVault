import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type {
  EditCommandError,
  EditFieldDto,
  EditSavedDto,
  EditValueDto,
  ItemEditDto,
} from "../../bindings";
import { createSyntheticEditBackend } from "./syntheticEdit";

export type EditField = EditFieldDto;
export type EditValue = EditValueDto;
export type ItemEdit = ItemEditDto;
export type EditSaved = EditSavedDto;

/** The kinds of item the detail pages edit. A season is edited through its show. */
export type EditKind = "movie" | "show" | "episode";

/** The item an edit call is about. The kind only matters to the browser fixture. */
export interface EditTarget {
  id: string;
  kind: EditKind;
}

/** A refusal from the backend: `message` is ready to show; `field` names the refused field. */
export class EditRequestError extends Error {
  readonly code: string;
  readonly field: string | null;

  constructor(code: string, message: string, field: string | null = null) {
    super(message);
    this.name = "EditRequestError";
    this.code = code;
    this.field = field;
  }
}

/** Where the detail pages edit: the database in the desktop app, a fixture in a browser. */
export interface EditBackend {
  /** Every editable field of the item with its lock key, the locked fields and the item lock. */
  load: (target: EditTarget) => Promise<ItemEdit>;
  /**
   * Saves the fields, all or nothing, and rewrites the item's NFO files. A refused value rejects
   * with an {@link EditRequestError} naming the field and saves nothing.
   */
  save: (target: EditTarget, changes: readonly EditField[]) => Promise<EditSaved>;
  /** Locks or unlocks a field (by its lock key) against scraping. */
  setFieldLock: (target: EditTarget, lockKey: string, locked: boolean) => Promise<void>;
  /** Locks or unlocks the whole item. */
  setItemLock: (target: EditTarget, locked: boolean) => Promise<void>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: EditCommandError };

function unwrap<T>(result: Result<T>): T {
  if (result.status === "error") {
    throw new EditRequestError(result.error.code, result.error.message, result.error.field);
  }
  return result.data;
}

const tauriEditBackend: EditBackend = {
  load: async ({ id }) => unwrap(await commands.getItemEdit(id)),
  save: async ({ id }, changes) => unwrap(await commands.saveItemEdit(id, [...changes])),
  setFieldLock: async ({ id }, lockKey, locked) => {
    unwrap(await commands.setFieldLock(id, lockKey, locked));
  },
  setItemLock: async ({ id }, locked) => {
    unwrap(await commands.setItemLock(id, locked));
  },
};

let override: EditBackend | null = null;
let fallback: EditBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setEditBackend(backend: EditBackend | null): void {
  override = backend;
}

export function currentEditBackend(): EditBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriEditBackend;
  // A plain browser has no database: edits go to an in-memory copy of the fixture.
  fallback ??= createSyntheticEditBackend();
  return fallback;
}
