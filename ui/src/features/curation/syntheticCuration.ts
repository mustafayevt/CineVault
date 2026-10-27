import type { CurationBackend } from "./curationBackend";

/** A plain browser has no library, so the NEW flag changes nothing there. */
export function createSyntheticCurationBackend(): CurationBackend {
  return {
    available: false,
    clearNew: () => Promise.resolve([]),
    clearNewOnOpen: () => Promise.resolve([]),
    clearAllNew: () => Promise.resolve({ changed: [], titles: 0 }),
    restoreNew: () => Promise.resolve(),
  };
}
