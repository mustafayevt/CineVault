import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";

/** The stored settings and a message the shell must show once (a damaged file was set aside). */
export interface LoadedSettings {
  values: Record<string, unknown>;
  notice: string | null;
}

/**
 * Where settings are kept. In the desktop app this is the atomic settings file behind typed
 * Tauri commands; in a plain browser (development, tests) it is `localStorage`, or memory when
 * that is unavailable.
 */
export interface SettingsBackend {
  load: () => Promise<LoadedSettings>;
  set: (key: string, value: unknown) => Promise<void>;
  reset: (keys: readonly string[]) => Promise<void>;
}

function parseJsonText(text: string): { ok: true; value: unknown } | { ok: false } {
  try {
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch {
    return { ok: false };
  }
}

const tauriBackend: SettingsBackend = {
  load: async () => {
    const snapshot = await commands.getSettings();
    const values: Record<string, unknown> = {};
    for (const [key, text] of Object.entries(snapshot.values)) {
      const parsed = parseJsonText(text);
      if (parsed.ok) values[key] = parsed.value;
    }
    return { values, notice: snapshot.notice };
  },
  set: async (key, value) => {
    const result = await commands.setSetting(key, JSON.stringify(value));
    if (result.status === "error") throw new Error(result.error);
  },
  reset: async (keys) => {
    const result = await commands.resetSettings([...keys]);
    if (result.status === "error") throw new Error(result.error);
  },
};

const BROWSER_KEY = "cinevault.settings.v1";

/** `localStorage` with a per-key read-modify-write, so a write before the load cannot clobber. */
function createBrowserBackend(): SettingsBackend {
  let fallback: Record<string, unknown> = {};
  const read = (): Record<string, unknown> => {
    try {
      const raw = window.localStorage.getItem(BROWSER_KEY);
      const parsed: unknown = raw === null ? {} : JSON.parse(raw);
      return typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
        ? { ...(parsed as Record<string, unknown>) }
        : {};
    } catch {
      return { ...fallback };
    }
  };
  const write = (values: Record<string, unknown>) => {
    fallback = values;
    try {
      window.localStorage.setItem(BROWSER_KEY, JSON.stringify(values));
    } catch {
      // Storage can be unavailable; settings then last for this session only.
    }
  };
  return {
    load: () => Promise.resolve({ values: read(), notice: null }),
    set: (key, value) => {
      write({ ...read(), [key]: value });
      return Promise.resolve();
    },
    reset: (keys) => {
      const values = read();
      for (const key of keys) Reflect.deleteProperty(values, key);
      write(values);
      return Promise.resolve();
    },
  };
}

/** A backend that keeps everything in memory. Used by tests. */
export function createMemoryBackend(initial: Record<string, unknown> = {}): SettingsBackend & {
  readonly stored: Record<string, unknown>;
} {
  const stored: Record<string, unknown> = { ...initial };
  return {
    stored,
    load: () => Promise.resolve({ values: { ...stored }, notice: null }),
    set: (key, value) => {
      stored[key] = value;
      return Promise.resolve();
    },
    reset: (keys) => {
      for (const key of keys) Reflect.deleteProperty(stored, key);
      return Promise.resolve();
    },
  };
}

let override: SettingsBackend | null = null;
let browser: SettingsBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. */
export function setSettingsBackend(backend: SettingsBackend | null): void {
  override = backend;
}

export function currentBackend(): SettingsBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriBackend;
  browser ??= createBrowserBackend();
  return browser;
}
