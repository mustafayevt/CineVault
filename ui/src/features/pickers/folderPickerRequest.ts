/** The event the folder picker host listens for, and the promise the caller waits on. */
export const PICK_EVENT = "cinevault:pick-folder";

export interface PickRequest {
  title?: string;
  resolve: (paths: string[]) => void;
}

export function requestFolderPick(title?: string): Promise<string[]> {
  return new Promise((resolve) => {
    window.dispatchEvent(
      new CustomEvent<PickRequest>(PICK_EVENT, { detail: { title, resolve } }),
    );
  });
}
