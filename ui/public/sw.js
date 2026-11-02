// The service worker of the browser version (spec `web-server` "Faster start over HTTPS").
// Registered only when the server is reached over HTTPS. It keeps the hashed build files for a
// faster start and shows a "Can't reach CineVault" page when the server can't be reached. It
// never stores a page, a command, an image or a media file: those belong to a signed-in session.

const CACHE = "cinevault-assets-v1";
const OFFLINE = "cinevault-offline-v1";
const OFFLINE_PAGE = new URL("/offline.html", self.location.origin).href;
/** Old builds' files are dropped beyond this many, oldest first. */
const MAX_ASSETS = 80;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(OFFLINE)
      .then((cache) => cache.add(new Request(OFFLINE_PAGE, { cache: "reload" })))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((names) =>
        Promise.all(
          names
            .filter((name) => name.startsWith("cinevault-") && name !== CACHE && name !== OFFLINE)
            .map((name) => caches.delete(name)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

/** A hashed build file of this server: its name changes with its content, so it never goes stale. */
function isHashedAsset(url) {
  return url.origin === self.location.origin && url.pathname.startsWith("/assets/");
}

async function trim(cache) {
  const keys = await cache.keys();
  await Promise.all(
    keys.slice(0, Math.max(0, keys.length - MAX_ASSETS)).map((key) => cache.delete(key)),
  );
}

async function fromCacheFirst(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  if (hit) return hit;
  const answer = await fetch(request);
  if (answer.ok && answer.type === "basic") {
    await cache.put(request, answer.clone());
    await trim(cache);
  }
  return answer;
}

async function pageOrOffline(request) {
  try {
    return await fetch(request);
  } catch {
    const offline = await caches.match(OFFLINE_PAGE, { cacheName: OFFLINE });
    return offline ?? Response.error();
  }
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (isHashedAsset(url)) {
    event.respondWith(fromCacheFirst(request));
  } else if (request.mode === "navigate" && url.origin === self.location.origin) {
    event.respondWith(pageOrOffline(request));
  }
  // Everything else (commands, streams, images, media) goes to the network untouched.
});
