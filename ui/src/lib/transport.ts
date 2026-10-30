/**
 * Which surface the interface is running on.
 *
 * `isTauri()` answers "is there a real command backend"; in server mode that is true, so feature
 * backends take the real path instead of the synthetic one. `transport()` answers "is this the
 * desktop app or a browser"; the few controls that only make sense in a desktop window use it.
 */

import { isTauri } from "@tauri-apps/api/core";

export type Transport = "desktop" | "browser";

export function transport(): Transport {
  const flag = (window as unknown as Record<string, unknown>).__CINEVAULT_TRANSPORT__;
  // The server installs its transport and sets `isTauri` true for the backends; the flag wins.
  if (flag === "server") return "browser";
  return isTauri() ? "desktop" : "browser";
}

export function isBrowser(): boolean {
  return transport() === "browser";
}

/** True only when the server injected its transport; a development browser is not the server. */
export function isServer(): boolean {
  return (window as unknown as Record<string, unknown>).__CINEVAULT_TRANSPORT__ === "server";
}
