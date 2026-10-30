/**
 * Browser transport for the CineVault server.
 *
 * The UI reaches the backend through `@tauri-apps/api`: `invoke` calls
 * `window.__TAURI_INTERNALS__.invoke`, `isTauri()` reads `globalThis.isTauri`, images go through
 * `convertFileSrc`, and streams use the real `Channel` class. Installing that one object with an
 * HTTP/SSE implementation is enough for the whole app to work unchanged, so no feature needs a
 * second backend.
 *
 * The server injects `window.__CINEVAULT_TRANSPORT__ = "server"` into `index.html`. Without it
 * (development, the gallery, screenshots, tests) this module does nothing and the synthetic
 * backends stay in use.
 */

const CLIENT_KEY = "cinevault.server.client";

type Callback = (payload: unknown) => void;

type ServerMessage =
  | { kind: "channel"; stream: number; index: number; message: unknown }
  | { kind: "end"; stream: number; index: number }
  | { kind: "event"; event: string; id?: number; payload: unknown }
  | { kind: "reset"; stream?: number; reason?: string };

function isServerMode(): boolean {
  return (window as unknown as Record<string, unknown>).__CINEVAULT_TRANSPORT__ === "server";
}

function randomId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function clientId(): string {
  const stored = sessionStorage.getItem(CLIENT_KEY);
  if (stored !== null) return stored;
  const id = randomId();
  sessionStorage.setItem(CLIENT_KEY, id);
  return id;
}

function csrfToken(): string {
  const value = (window as unknown as Record<string, unknown>).__CINEVAULT_CSRF__;
  return typeof value === "string" ? value : "";
}

function install(): void {
  const callbacks = new Map<number, Callback>();
  let nextCallbackId = 1;

  const eventListeners = new Map<string, Map<number, number>>();
  let nextEventId = 1;

  const client = clientId();
  let source: EventSource | null = null;

  const dispatch = (message: ServerMessage): void => {
    switch (message.kind) {
      case "channel": {
        callbacks.get(message.stream)?.({ index: message.index, message: message.message });
        break;
      }
      case "end": {
        callbacks.get(message.stream)?.({ index: message.index, end: true });
        break;
      }
      case "event": {
        for (const callbackId of eventListeners.get(message.event)?.values() ?? []) {
          callbacks.get(callbackId)?.({
            event: message.event,
            id: message.id ?? 0,
            payload: message.payload,
          });
        }
        break;
      }
      case "reset":
        window.dispatchEvent(new CustomEvent("cinevault:stream-reset", { detail: message }));
        break;
    }
  };

  const connect = (): void => {
    source = new EventSource(`/api/stream?client=${encodeURIComponent(client)}`);
    source.onmessage = (event) => {
      try {
        dispatch(JSON.parse(event.data) as ServerMessage);
      } catch {
        // A malformed message must not take the stream down.
      }
    };
    source.onerror = () => {
      // EventSource reconnects by itself unless it has given up; only recreate then.
      if (source?.readyState === EventSource.CLOSED) {
        window.setTimeout(connect, 3000);
      }
    };
  };

  const invoke = async (cmd: string, args: Record<string, unknown> = {}): Promise<unknown> => {
    if (cmd === "plugin:event|listen") {
      const event = String(args.event);
      const handler = Number(args.handler);
      const eventId = nextEventId++;
      let listeners = eventListeners.get(event);
      if (listeners === undefined) {
        listeners = new Map();
        eventListeners.set(event, listeners);
      }
      listeners.set(eventId, handler);
      return eventId;
    }
    if (cmd === "plugin:event|unlisten") {
      const event = String(args.event);
      eventListeners.get(event)?.delete(Number(args.eventId));
      return null;
    }
    // A browser has no window or webview zoom; these are desktop-only and are hidden already.
    if (cmd === "plugin:window|is_maximized") return false;
    if (cmd === "plugin:webview|set_webview_zoom") return null;

    const response = await fetch("/api/invoke", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-cinevault-csrf": csrfToken(),
      },
      body: JSON.stringify({ cmd, args, client }),
    });
    if (response.status === 401) {
      window.dispatchEvent(new CustomEvent("cinevault:signed-out"));
      // `typedError` rethrows an `Error`; reject with the typed value so it becomes a command
      // error the UI can branch on.
      throw { code: "signed_out", message: "Your session ended. Sign in again." };
    }
    const answer = (await response.json()) as
      { ok: true; data: unknown } | { ok: false; error: unknown };
    if (!answer.ok) throw answer.error;
    return answer.data;
  };

  const internals = {
    invoke,
    transformCallback: (callback: Callback, once = false): number => {
      const id = nextCallbackId++;
      callbacks.set(id, (payload) => {
        if (once) callbacks.delete(id);
        callback(payload);
      });
      return id;
    },
    unregisterCallback: (id: number): void => {
      callbacks.delete(id);
    },
    // `imageUrl` passes `img/<key>/<size>`; the server serves it at `/img/<key>/<size>`.
    convertFileSrc: (path: string): string => `/${path}`,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
  };

  const global = window as unknown as Record<string, unknown>;
  global.__TAURI_INTERNALS__ = internals;
  global.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (event: string, eventId: number): void => {
      eventListeners.get(event)?.delete(eventId);
    },
  };
  // The feature backends' `isTauri()` checks now take the real command path.
  global.isTauri = true;

  connect();
}

if (isServerMode()) install();

/** The client id this page sends, or null outside the server. */
export function serverClientId(): string | null {
  return isServerMode() ? sessionStorage.getItem(CLIENT_KEY) : null;
}
