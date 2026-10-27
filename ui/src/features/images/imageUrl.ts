import { convertFileSrc } from "@tauri-apps/api/core";

import { syntheticImageUrl } from "./syntheticImages";

/**
 * Sizes the `cv://img` protocol serves (thumbnails are cached on disk by size). `full` is the
 * original file itself, for a picture that fills the window (the title page's backdrop): no
 * thumbnail is sharp enough at 2560 pixels and twice the pixel density. Without a readable
 * original it answers with the hero thumbnail.
 */
export type ThumbSize = "grid" | "list" | "hero" | "full";

/** The custom URI scheme the desktop app registers for images. */
const IMAGE_SCHEME = "cv";

/** A cache key is 32 lower-case hex characters: the hash of the image content. */
const KEY_PATTERN = /^[0-9a-f]{32}$/;

export function isImageKey(key: string): boolean {
  return KEY_PATTERN.test(key);
}

/**
 * The URL of a cached thumbnail, for `<img src>` and CSS `url()`.
 *
 * Images never travel over IPC: the web view fetches this URL itself, and the Rust protocol
 * handler answers with long-lived cache headers, generating the thumbnail on first request.
 * Inside the desktop app Tauri picks the platform form (`http://cv.localhost/...` on Windows and
 * Android, `cv://localhost/...` elsewhere); a plain browser gets the `cv://` form.
 */
export function imageUrl(
  key: string,
  size: ThumbSize,
  options: { preview?: boolean } = {},
): string {
  if (!isImageKey(key)) {
    throw new Error(`Not an image key: ${key}`);
  }
  const path = `img/${key}/${size}`;
  // A candidate shown before its file is written gets a URL of its own (the protocol ignores the
  // query): the stand-in it is served then is not reused once the real size can be made.
  const query = options.preview === true ? "?preview" : "";
  const internals = (window as { __TAURI_INTERNALS__?: { convertFileSrc?: unknown } })
    .__TAURI_INTERNALS__;
  if (typeof internals?.convertFileSrc === "function") {
    return `${convertFileSrc(path, IMAGE_SCHEME)}${query}`;
  }
  // A plain browser has no protocol handler: the fixture pictures are drawn in place.
  return syntheticImageUrl(key) ?? `${IMAGE_SCHEME}://localhost/${path}${query}`;
}
