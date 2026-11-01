import type { ShareSupport } from "./sharesBackend";

/** What the Address field holds: a share, a folder inside it, and the user an `smb://` address names. */
export interface ShareAddress {
  server: string;
  share: string;
  /** The folder inside the share, one name per level; empty for the share itself. */
  folder: readonly string[];
  user: string | null;
}

function decoded(part: string): string {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

/**
 * Reads `\\nas\movies\Films`, `//nas/movies/Films` and `smb://[user@]nas/movies/Films`, so the page
 * can show the library folder as it is typed. Only the shape is read here; the backend checks
 * every part when "Connect and add" is pressed. `null` while there is no server and share yet.
 */
export function parseShareAddress(text: string): ShareAddress | null {
  const trimmed = text.trim();
  let user: string | null = null;
  let rest: string;
  let decode = false;
  if (/^smb:\/\//i.test(trimmed)) {
    decode = true;
    rest = trimmed.slice(6);
    const end = rest.includes("/") ? rest.indexOf("/") : rest.length;
    const at = rest.slice(0, end).lastIndexOf("@");
    if (at >= 0) {
      const named = decoded(rest.slice(0, at));
      user = named.includes(";") ? named.replace(";", "\\") : named;
      rest = rest.slice(at + 1);
    }
  } else if (trimmed.startsWith("\\\\") || trimmed.startsWith("//")) {
    rest = trimmed.slice(2);
  } else {
    return null;
  }
  const [server, share, ...folder] = rest.replace(/[\\/]+$/, "").split(/[\\/]/);
  if (server === undefined || server === "" || share === undefined || share === "") return null;
  const part = (value: string) => (decode ? decoded(value) : value);
  return {
    server,
    share: part(share),
    folder: folder.filter((name) => name !== "").map(part),
    user,
  };
}

/** The share as it is written everywhere: `\\nas\movies`. */
export function shareName(address: ShareAddress): string {
  return `\\\\${address.server}\\${address.share}`;
}

/**
 * Where the library folder will be on this computer: the network path on Windows, a folder of
 * CineVault's own mount folder on macOS, the desktop's own place for user-mounted shares on Linux.
 */
export function libraryFolderPath(address: ShareAddress, support: ShareSupport): string {
  if (support.system === "windows" || support.mount_base === null) {
    return [shareName(address), ...address.folder].join("\\");
  }
  const share =
    support.system === "linux"
      ? `smb-share:server=${address.server.toLowerCase()},share=${address.share.toLowerCase()}`
      : `${address.server}/${address.share}`;
  return [support.mount_base, share, ...address.folder].join("/");
}
