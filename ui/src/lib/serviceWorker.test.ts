import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";

import { registerServiceWorker } from "./serviceWorker";
import type { ServiceWorkerHost } from "./serviceWorker";

function host(over: Partial<ServiceWorkerHost> = {}) {
  const register = vi.fn(() => Promise.resolve({} as ServiceWorkerRegistration));
  return {
    register,
    host: {
      isSecureContext: true,
      protocol: "https:",
      serviceWorker: { register },
      server: true,
      ...over,
    },
  };
}

describe("registering the service worker", () => {
  it("registers in the browser version over HTTPS", () => {
    const { host: h, register } = host();
    expect(registerServiceWorker(h)).toBe(true);
    expect(register).toHaveBeenCalledWith("/sw.js", { scope: "/" });
  });

  it("does not register over plain HTTP, in the desktop app or without service workers", () => {
    for (const over of [
      { protocol: "http:", isSecureContext: false },
      // `http://localhost` is a secure context, but it is a development address, not HTTPS.
      { protocol: "http:" },
      { server: false },
      { serviceWorker: undefined },
    ]) {
      const { host: h, register } = host(over);
      expect(registerServiceWorker(h), JSON.stringify(over)).toBe(false);
      expect(register).not.toHaveBeenCalled();
    }
  });
});

// ---- the worker itself, run against a small stand-in for the service worker scope -----------

class FakeCache {
  entries = new Map<string, Response>();
  match(request: Request | string) {
    const url = typeof request === "string" ? new URL(request, ORIGIN).href : request.url;
    return Promise.resolve(this.entries.get(url)?.clone());
  }
  put(request: Request, response: Response) {
    this.entries.set(request.url, response);
    return Promise.resolve();
  }
  add(request: Request) {
    return fetchFake(request).then((response) => this.put(request, response));
  }
  keys() {
    return Promise.resolve([...this.entries.keys()].map((url) => new Request(url)));
  }
  delete(request: Request) {
    return Promise.resolve(this.entries.delete(request.url));
  }
}

const ORIGIN = "https://nas.example";
let online = true;
const fetchFake = vi.fn((request: Request) =>
  online
    ? Promise.resolve(
        Object.defineProperty(new Response(`body of ${request.url}`), "type", { value: "basic" }),
      )
    : Promise.reject(new TypeError("Failed to fetch")),
);

function loadWorker() {
  const stores = new Map<string, FakeCache>();
  const listeners = new Map<string, (event: unknown) => void>();
  const caches = {
    open: (name: string) => {
      if (!stores.has(name)) stores.set(name, new FakeCache());
      return Promise.resolve(stores.get(name));
    },
    keys: () => Promise.resolve([...stores.keys()]),
    delete: (name: string) => Promise.resolve(stores.delete(name)),
    match: (request: string, options: { cacheName: string }) =>
      stores.get(options.cacheName)?.match(request) ?? Promise.resolve(undefined),
  };
  const self = {
    location: new URL(`${ORIGIN}/sw.js`),
    addEventListener: (type: string, handler: (event: unknown) => void) =>
      listeners.set(type, handler),
    skipWaiting: () => Promise.resolve(),
    clients: { claim: () => Promise.resolve() },
  };
  const source = readFileSync(resolve(__dirname, "..", "..", "public", "sw.js"), "utf8");
  // Runs the shipped worker file as it is.
  new Function("self", "caches", "fetch", source)(self, caches, fetchFake);

  async function lifecycle(type: "install" | "activate") {
    let wait: Promise<unknown> = Promise.resolve();
    listeners.get(type)?.({ waitUntil: (promise: Promise<unknown>) => (wait = promise) });
    await wait;
  }
  async function request(
    path: string,
    init: { mode?: string; method?: string } = {},
  ): Promise<Response | null> {
    const request = new Request(`${ORIGIN}${path}`, { method: init.method ?? "GET" });
    Object.defineProperty(request, "mode", { value: init.mode ?? "cors" });
    let answer: Promise<Response> | null = null;
    listeners.get("fetch")?.({
      request,
      respondWith: (value: Promise<Response>) => (answer = value),
    });
    return answer === null ? null : await answer;
  }
  return { stores, lifecycle, request };
}

describe("the service worker", () => {
  it("keeps the hashed build files and serves them when the server is away", async () => {
    online = true;
    const worker = loadWorker();
    await worker.lifecycle("install");
    await worker.lifecycle("activate");
    expect(await (await worker.request("/assets/index-abc123.js"))?.text()).toBe(
      `body of ${ORIGIN}/assets/index-abc123.js`,
    );
    online = false;
    expect(await (await worker.request("/assets/index-abc123.js"))?.text()).toBe(
      `body of ${ORIGIN}/assets/index-abc123.js`,
    );
  });

  it("shows the offline page for a page the server can't answer", async () => {
    online = true;
    const worker = loadWorker();
    await worker.lifecycle("install");
    online = false;
    const page = await worker.request("/movies/42", { mode: "navigate" });
    expect(await page?.text()).toBe(`body of ${ORIGIN}/offline.html`);
  });

  it("leaves pages, commands, images and media to the network and stores none of them", async () => {
    online = true;
    const worker = loadWorker();
    await worker.lifecycle("install");
    expect(await worker.request("/api/invoke", { method: "POST" })).toBeNull();
    expect(await worker.request("/img/0123456789abcdef0123456789abcdef/poster")).toBeNull();
    expect(await worker.request("/media/42/1")).toBeNull();
    expect(await worker.request("/api/stream")).toBeNull();
    const page = await worker.request("/library", { mode: "navigate" });
    expect(await page?.text()).toBe(`body of ${ORIGIN}/library`);
    const stored = [...worker.stores.values()].flatMap((cache) => [...cache.entries.keys()]);
    expect(stored).toEqual([`${ORIGIN}/offline.html`]);
  });

  it("drops the files of old builds once there are many", async () => {
    online = true;
    const worker = loadWorker();
    for (let n = 0; n < 90; n += 1) await worker.request(`/assets/chunk-${n}.js`);
    const assets = worker.stores.get("cinevault-assets-v1");
    expect(assets?.entries.size).toBe(80);
    expect(assets?.entries.has(`${ORIGIN}/assets/chunk-0.js`)).toBe(false);
    expect(assets?.entries.has(`${ORIGIN}/assets/chunk-89.js`)).toBe(true);
  });

  it("removes caches of an earlier worker version when it takes over", async () => {
    online = true;
    const worker = loadWorker();
    worker.stores.set("cinevault-assets-v0", new FakeCache());
    await worker.lifecycle("install");
    await worker.lifecycle("activate");
    expect(worker.stores.has("cinevault-assets-v0")).toBe(false);
    expect(worker.stores.has("cinevault-offline-v1")).toBe(true);
  });
});
