import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { currentBackend } from "./settingsBackend";
import { isDefaultValue, isValidValue, settingDef } from "./schema";
import type { SettingDef, SettingValue } from "./schema";

/** Settings kept outside the page's schema: other features store their own state under a key. */
export const SIDEBAR_SETTING_KEY = "shell.sidebar";

type Values = Readonly<Record<string, unknown>>;

export interface SettingsState {
  /** What differs from the defaults, by id. A schema setting holds a value only when modified. */
  values: Values;
  status: "idle" | "loading" | "ready";
  /** Reads the stored settings. Safe to call more than once. */
  load: () => Promise<void>;
  /** Changes a setting at once and saves it. Setting the default clears it. */
  set: (id: string, value: unknown) => void;
  /** Returns settings to their defaults (or removes the other features' keys). */
  reset: (ids: readonly string[]) => void;
  /** Puts back values captured before a reset: the Undo of a reset. */
  restore: (previous: Values) => void;
}

function sameValue(a: unknown, b: unknown): boolean {
  return a === b || JSON.stringify(a) === JSON.stringify(b);
}

/** Saves run one after another so the file always ends with the last change. */
let saveQueue: Promise<void> = Promise.resolve();

function save(title: string, run: () => Promise<void>, rollback: () => void): void {
  saveQueue = saveQueue.then(run).catch((error: unknown) => {
    rollback();
    notify({
      severity: "error",
      title,
      detail: t("settings.changeUndone", {
        reason: error instanceof Error ? error.message : String(error),
      }),
      source: t("settings.source"),
    });
  });
}

/** Resolves when every save made so far has finished. */
export function settleSettingSaves(): Promise<void> {
  return saveQueue;
}

/** The value to store for `id`, or `undefined` to store nothing (the default). */
function storable(id: string, value: unknown): unknown {
  const def = settingDef(id);
  return def !== undefined && isDefaultValue(def, value) ? undefined : value;
}

export const useSettingsStore = create<SettingsState>((set, get) => {
  /** Applies `next` values for `ids` in memory and saves the difference; undoes it on failure. */
  const apply = (changes: Record<string, unknown>, failure: string) => {
    const before = get().values;
    const after: Record<string, unknown> = { ...before };
    const written: string[] = [];
    const removed: string[] = [];
    for (const [id, value] of Object.entries(changes)) {
      const next = storable(id, value);
      if (sameValue(before[id], next)) continue;
      if (next === undefined) {
        Reflect.deleteProperty(after, id);
        removed.push(id);
      } else {
        after[id] = next;
        written.push(id);
      }
    }
    if (written.length === 0 && removed.length === 0) return;
    set({ values: after });
    const rollback = () =>
      set((state) => {
        const values: Record<string, unknown> = { ...state.values };
        for (const id of [...written, ...removed]) {
          if (Object.hasOwn(before, id)) values[id] = before[id];
          else Reflect.deleteProperty(values, id);
        }
        return { values };
      });
    const backend = currentBackend();
    save(
      failure,
      async () => {
        if (removed.length > 0) await backend.reset(removed);
        for (const id of written) await backend.set(id, after[id]);
      },
      rollback,
    );
  };

  return {
    values: {},
    status: "idle",
    load: async () => {
      if (get().status !== "idle") return;
      set({ status: "loading" });
      try {
        const loaded = await currentBackend().load();
        // A change made while loading wins over the stored value.
        set((state) => ({ values: { ...loaded.values, ...state.values }, status: "ready" }));
        if (loaded.notice !== null) {
          notify({
            severity: "warning",
            title: t("settings.unreadable"),
            detail: loaded.notice,
            source: t("settings.source"),
          });
        }
      } catch (error) {
        set({ status: "ready" });
        notify({
          severity: "error",
          title: t("settings.loadFailed"),
          detail: error instanceof Error ? error.message : String(error),
          source: t("settings.source"),
        });
      }
    },
    set: (id, value) => apply({ [id]: value }, t("settings.saveFailed")),
    reset: (ids) =>
      apply(Object.fromEntries(ids.map((id) => [id, undefined])), t("settings.resetFailed")),
    restore: (previous) => apply({ ...previous }, t("settings.restoreFailed")),
  };
});

/** The value in effect: the stored one when it is legal, else the default. */
function effectiveValue<V extends SettingValue>(values: Values, def: SettingDef): V {
  const stored = Object.hasOwn(values, def.id) ? values[def.id] : undefined;
  return (isValidValue(def, stored) ? stored : def.default) as V;
}

/** The value of a setting from the schema, live. */
export function useSetting<V extends SettingValue>(id: string): V {
  const def = settingDef(id);
  if (def === undefined) throw new Error(`Unknown setting: ${id}`);
  return useSettingsStore((state) => effectiveValue<V>(state.values, def));
}

/** The value of a setting from the schema, read once (for actions outside a component). */
export function readSetting<V extends SettingValue>(id: string): V {
  const def = settingDef(id);
  if (def === undefined) throw new Error(`Unknown setting: ${id}`);
  return effectiveValue<V>(useSettingsStore.getState().values, def);
}
