import { isServer } from "./transport";

/** What registering needs from the window; the real window by default, a stand-in in tests. */
export interface ServiceWorkerHost {
  isSecureContext: boolean;
  protocol: string;
  serviceWorker: Pick<ServiceWorkerContainer, "register"> | undefined;
  server: boolean;
}

function windowHost(): ServiceWorkerHost {
  return {
    isSecureContext: window.isSecureContext,
    protocol: window.location.protocol,
    serviceWorker: "serviceWorker" in navigator ? navigator.serviceWorker : undefined,
    server: isServer(),
  };
}

/**
 * Registers `/sw.js` (spec `web-server` "Faster start over HTTPS"): only in the browser version,
 * only over HTTPS and only where the browser has service workers. The desktop app, a development
 * browser and a plain `http://` home-network address go without one, as before.
 */
export function registerServiceWorker(host: ServiceWorkerHost = windowHost()): boolean {
  if (!host.server || !host.isSecureContext || host.protocol !== "https:") return false;
  if (host.serviceWorker === undefined) return false;
  void host.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => undefined);
  return true;
}
