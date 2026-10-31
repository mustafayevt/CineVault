import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { BrowserAccessDto, BrowserAccessError } from "../../bindings";

export type BrowserAccess = BrowserAccessDto;
export type BrowserSession = BrowserAccessDto["sessions"][number];

/** A refusal from the desktop: `message` is ready to show, `code` is stable. */
export class BrowserAccessRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "BrowserAccessRequestError";
    this.code = code;
  }
}

/**
 * The desktop's Browser access: the server the app runs inside itself so other devices on the
 * home network can open the library (spec `settings` "Browser access section"). Every call
 * answers with the whole state, so the section never shows half a change.
 */
export interface BrowserAccessBackend {
  get: () => Promise<BrowserAccess>;
  setEnabled: (on: boolean) => Promise<BrowserAccess>;
  setPort: (port: number) => Promise<BrowserAccess>;
  setLanOnly: (on: boolean) => Promise<BrowserAccess>;
  setTray: (on: boolean) => Promise<BrowserAccess>;
  setStartAtLogin: (on: boolean) => Promise<BrowserAccess>;
  setPassword: (password: string, confirm: string) => Promise<BrowserAccess>;
  /** One browser by its handle, or every browser with `null`. */
  signOut: (handle: string | null) => Promise<BrowserAccess>;
}

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: BrowserAccessError };

function unwrap(result: Result<BrowserAccess>): BrowserAccess {
  if (result.status === "error") {
    throw new BrowserAccessRequestError(result.error.code, result.error.message);
  }
  return result.data;
}

const tauriBackend: BrowserAccessBackend = {
  get: async () => unwrap(await commands.getBrowserAccess()),
  setEnabled: async (on) => unwrap(await commands.setBrowserAccessEnabled(on)),
  setPort: async (port) => unwrap(await commands.setBrowserAccessPort(port)),
  setLanOnly: async (on) => unwrap(await commands.setBrowserAccessLanOnly(on)),
  setTray: async (on) => unwrap(await commands.setBrowserAccessTray(on)),
  setStartAtLogin: async (on) => unwrap(await commands.setStartAtLogin(on)),
  setPassword: async (password, confirm) =>
    unwrap(await commands.setBrowserAccessPassword(password, confirm)),
  signOut: async (handle) => unwrap(await commands.signOutBrowsers(handle)),
};

/**
 * The development backend (`bun run dev`, the gallery, tests): the same answers in memory, with
 * a sample computer and two signed-in browsers once it runs.
 */
export function createSyntheticBrowserAccess(
  start: Partial<BrowserAccess> = {},
): BrowserAccessBackend {
  const now = Date.now();
  const sample: BrowserSession[] = [
    {
      handle: "a1",
      address: "192.168.1.31",
      agent:
        "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
      signedInMs: now - 3_600_000,
      lastUsedMs: now - 60_000,
    },
  ];
  let password: string | null = start.hasPassword === true ? "a sample password" : null;
  let state: BrowserAccess = {
    available: true,
    enabled: false,
    state: "off",
    message: null,
    port: 8787,
    addresses: [],
    lanOnly: true,
    hasPassword: false,
    sessions: start.enabled === true ? sample : [],
    keepInTray: false,
    startAtLogin: false,
    windows: true,
    computer: "STUDY-PC",
    ...start,
  };
  const running = (next: BrowserAccess): BrowserAccess => {
    const on = next.enabled && password !== null;
    return {
      ...next,
      hasPassword: password !== null,
      state: on ? "running" : next.enabled ? "noPassword" : "off",
      addresses: on ? [`http://192.168.1.20:${next.port}`, `http://localhost:${next.port}`] : [],
      sessions: on ? next.sessions : [],
    };
  };
  const answer = (next: BrowserAccess) => {
    state = running(next);
    return Promise.resolve(state);
  };
  state = running(state);
  return {
    get: () => Promise.resolve(state),
    setEnabled: (on) => {
      if (on && password === null) {
        return Promise.reject(
          new BrowserAccessRequestError("password", "Create a password first."),
        );
      }
      return answer({ ...state, enabled: on, sessions: on ? sample : [] });
    },
    setPort: (port) => answer({ ...state, port, sessions: [] }),
    setLanOnly: (on) => answer({ ...state, lanOnly: on, sessions: [] }),
    setTray: (on) => answer({ ...state, keepInTray: on }),
    setStartAtLogin: (on) => answer({ ...state, startAtLogin: on }),
    setPassword: (next, confirm) => {
      if (next.length < 12) {
        return Promise.reject(
          new BrowserAccessRequestError("password", "The password must be at least 12 characters."),
        );
      }
      if (next !== confirm) {
        return Promise.reject(
          new BrowserAccessRequestError("password", "The passwords do not match."),
        );
      }
      password = next;
      return answer({ ...state, sessions: [] });
    },
    signOut: (handle) =>
      answer({
        ...state,
        sessions: handle === null ? [] : state.sessions.filter((s) => s.handle !== handle),
      }),
  };
}

let override: BrowserAccessBackend | null = null;
let synthetic: BrowserAccessBackend | null = null;

/** The backend in use: the desktop's commands, or the synthetic one outside the app. */
export function currentBrowserAccessBackend(): BrowserAccessBackend {
  if (override !== null) return override;
  if (isTauri()) return tauriBackend;
  synthetic ??= createSyntheticBrowserAccess();
  return synthetic;
}

/** Tests choose the backend; `null` puts the default back. */
export function setBrowserAccessBackend(backend: BrowserAccessBackend | null): void {
  override = backend;
}

/**
 * A short name for a signed-in browser from its `User-Agent`: "Edge on Windows",
 * "Safari on iPhone". Unknown agents are "A browser".
 */
export function browserLabel(agent: string): { browser: string; system: string } | null {
  const browser = /Edg\//.test(agent)
    ? "Edge"
    : /OPR\/|Opera/.test(agent)
      ? "Opera"
      : /Firefox\//.test(agent)
        ? "Firefox"
        : /SamsungBrowser\//.test(agent)
          ? "Samsung Internet"
          : /Chrome\/|CriOS\//.test(agent)
            ? "Chrome"
            : /Safari\//.test(agent)
              ? "Safari"
              : null;
  const system = /iPhone/.test(agent)
    ? "iPhone"
    : /iPad/.test(agent)
      ? "iPad"
      : /Android/.test(agent)
        ? "Android"
        : /Windows/.test(agent)
          ? "Windows"
          : /Mac OS X|Macintosh/.test(agent)
            ? "macOS"
            : /CrOS/.test(agent)
              ? "ChromeOS"
              : /Linux/.test(agent)
                ? "Linux"
                : null;
  if (browser === null || system === null) return null;
  return { browser, system };
}
