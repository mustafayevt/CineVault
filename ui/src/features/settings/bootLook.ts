import type { MotionPreference, ThemePreference } from "./appearance";

/**
 * The look the window had when it last closed, kept in local storage so the next start draws
 * the right theme and motion before the settings file has been read: a light-theme user
 * never sees a dark frame first, and the zoom is back before the first screen. The small script in `index.html` reads the same
 * key before the first paint; the settings stay the source of truth and overwrite it once loaded.
 */
export const BOOT_LOOK_KEY = "cv.boot-look";

export interface BootLook {
  theme: ThemePreference;
  motion: MotionPreference;
  /** The app zoom in percent; a look saved before the zoom existed has none. */
  zoom?: number;
}

function isBootLook(value: unknown): value is BootLook {
  if (typeof value !== "object" || value === null) return false;
  const look = value as Partial<Record<keyof BootLook, unknown>>;
  return (
    (look.theme === "dark" || look.theme === "light" || look.theme === "system") &&
    (look.motion === "system" || look.motion === "reduced" || look.motion === "full") &&
    (look.zoom === undefined || (typeof look.zoom === "number" && Number.isFinite(look.zoom)))
  );
}

/** The saved look, or `null` on a first start or when storage is not available. */
export function readBootLook(): BootLook | null {
  try {
    const text = window.localStorage.getItem(BOOT_LOOK_KEY);
    if (text === null) return null;
    const parsed: unknown = JSON.parse(text);
    return isBootLook(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

/** Remembers the look for the next start. Storage that refuses the write is ignored. */
export function saveBootLook(look: BootLook): void {
  try {
    window.localStorage.setItem(BOOT_LOOK_KEY, JSON.stringify(look));
  } catch {
    // A private or full storage only costs the next start its first frame.
  }
}
