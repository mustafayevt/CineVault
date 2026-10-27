import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { PickFileKind } from "../../bindings";

export type { PickFileKind };

/** What a picker can choose: a folder, or files of one kind. */
export type PickKind = "folder" | PickFileKind;

/** Options of one pick. */
export interface PickOptions {
  /** The dialog's title. */
  title?: string;
  /** Files only: allow several. */
  multiple?: boolean;
}

/** The system pickers. They open from the desktop shell, so the web view needs no permission. */
export interface PickerBackend {
  /** False in a plain browser, where there is no system dialog; the Browse buttons stay hidden. */
  readonly available: boolean;
  /** The chosen paths; empty when the user cancels. A folder pick holds at most one. */
  pick: (kind: PickKind, options?: PickOptions) => Promise<string[]>;
}

const tauriBackend: PickerBackend = {
  available: true,
  pick: async (kind, options) => {
    if (kind === "folder") {
      const folder = await commands.pickFolder(options?.title ?? null);
      return folder === null ? [] : [folder];
    }
    return commands.pickFiles(kind, options?.multiple ?? false, options?.title ?? null);
  },
};

const unavailableBackend: PickerBackend = {
  available: false,
  pick: () => Promise.resolve([]),
};

/** A stand-in for a browser preview (`?first-run`): each pick answers with a sample path. */
function createSyntheticPickerBackend(): PickerBackend {
  return {
    available: true,
    pick: (kind) =>
      Promise.resolve(kind === "folder" ? ["D:\\Media\\Picked"] : ["D:\\Art\\poster.jpg"]),
  };
}

const override: PickerBackend | null = null;

let synthetic: PickerBackend | null = null;

export function currentPickerBackend(): PickerBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriBackend;
  if (new URLSearchParams(window.location.search).has("first-run")) {
    synthetic ??= createSyntheticPickerBackend();
    return synthetic;
  }
  return unavailableBackend;
}
