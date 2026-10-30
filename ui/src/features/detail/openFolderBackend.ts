import { commands } from "../../bindings";
import { transport } from "../../lib/transport";

/** Where the Open folder actions go: in the desktop app that is the Rust service. */
export interface OpenFolderBackend {
  /** Opens a folder in the file manager (the backend refuses anything it does not know). */
  open: (path: string) => Promise<void>;
}

const tauriBackend: OpenFolderBackend = {
  open: async (path) => {
    const result = await commands.openFolder(path);
    if (result.status === "error") throw new Error(result.error.message);
  },
};

/** In a plain browser (development and design review) nothing opens. */
const syntheticBackend: OpenFolderBackend = { open: () => Promise.resolve() };

let override: OpenFolderBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setOpenFolderBackend(backend: OpenFolderBackend | null): void {
  override = backend;
}

/** The backend in use: the override, the Rust service in the desktop app, else a no-op. */
export function currentOpenFolderBackend(): OpenFolderBackend {
  if (override !== null) return override;
  return transport() === "desktop" ? tauriBackend : syntheticBackend;
}
