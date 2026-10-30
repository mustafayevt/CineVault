/** The event the folder picker host listens for, and the promise the caller waits on. */
export const PICK_EVENT = "cinevault:pick-folder";

export interface PickRequest {
  title?: string;
  resolve: (paths: string[]) => void;
}

export function requestFolderPick(title?: string): Promise<string[]> {
  return new Promise((resolve) => {
    window.dispatchEvent(new CustomEvent<PickRequest>(PICK_EVENT, { detail: { title, resolve } }));
  });
}

/** Writes and removes a probe file in `path`; the server's answer for the setup rows. */
export async function probeFolder(path: string): Promise<{ writable: boolean; message?: string }> {
  const csrf = (window as unknown as Record<string, unknown>).__CINEVAULT_CSRF__;
  const response = await fetch("/api/fs/probe", {
    method: "POST",
    credentials: "same-origin",
    headers: {
      "content-type": "application/json",
      "x-cinevault-csrf": typeof csrf === "string" ? csrf : "",
    },
    body: JSON.stringify({ path }),
  });
  const body = (await response.json().catch(() => ({}))) as {
    writable?: boolean;
    message?: string;
  };
  return { writable: response.ok && body.writable === true, message: body.message };
}
