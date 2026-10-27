import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";

/**
 * Tells the image protocol where an image file lives and returns its content key. Only the path
 * goes in and only a key comes back; the thumbnail itself is fetched through `cv://img`.
 */
export async function registerImageSource(path: string): Promise<string> {
  if (!isTauri()) {
    throw new Error("Cached images are only available in the desktop app.");
  }
  const result = await commands.registerImageSource(path);
  if (result.status === "error") {
    throw new Error(result.error);
  }
  return result.data.key;
}
