/**
 * Which surface the interface is running on.
 *
 * `isTauri()` answers "is there a real command backend"; in server mode that is true, so feature
 * backends take the real path instead of the synthetic one. `transport()` answers "is this the
 * desktop app or a browser"; the few controls that only make sense in a desktop window use it.
 */

export type Transport = "desktop" | "browser";

export function transport(): Transport {
  const flag = (window as unknown as Record<string, unknown>).__CINEVAULT_TRANSPORT__;
  return flag === "server" ? "browser" : "desktop";
}

export function isBrowser(): boolean {
  return transport() === "browser";
}
