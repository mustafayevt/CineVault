import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { readBootLook } from "./bootLook";
import { useSettingsStore } from "./settingsStore";

/** The setting that holds the app zoom, in percent. */
export const ZOOM_SETTING_KEY = "appearance.zoom";

/** The zoom levels, in percent, from smallest to largest. */
export const ZOOM_STEPS: readonly number[] = [80, 90, 100, 110, 125, 150];

export const DEFAULT_ZOOM = 100;

export type ZoomAction = "in" | "out" | "reset";

/** The step nearest to `percent`, so a hand-edited settings file still lands on a step. */
export function snapZoom(percent: number): number {
  if (!Number.isFinite(percent)) return DEFAULT_ZOOM;
  let best = DEFAULT_ZOOM;
  let distance = Number.POSITIVE_INFINITY;
  for (const step of ZOOM_STEPS) {
    const gap = Math.abs(step - percent);
    if (gap < distance) {
      best = step;
      distance = gap;
    }
  }
  return best;
}

/** The zoom after `action`, clamped to the first and last step. */
export function nextZoom(current: number, action: ZoomAction): number {
  if (action === "reset") return DEFAULT_ZOOM;
  const index = ZOOM_STEPS.indexOf(snapZoom(current));
  const target = index + (action === "in" ? 1 : -1);
  return ZOOM_STEPS[Math.min(Math.max(target, 0), ZOOM_STEPS.length - 1)] ?? DEFAULT_ZOOM;
}

/**
 * The zoom action a key press stands for, or `null`. The plus key, the equals key it shares and
 * the keypad's plus zoom in (with or without Shift), minus and the keypad's minus zoom out, and 0
 * resets. The modifier is Ctrl, or Command on macOS; Alt makes it another chord.
 */
export function zoomActionForKey(
  event: Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey">,
  mac: boolean,
): ZoomAction | null {
  if (event.altKey || !(mac ? event.metaKey : event.ctrlKey)) return null;
  if (event.key === "+" || event.key === "=" || event.code === "NumpadAdd") return "in";
  if (event.key === "-" || event.key === "_" || event.code === "NumpadSubtract") return "out";
  if (event.key === "0" || event.code === "Numpad0" || event.code === "Digit0") return "reset";
  return null;
}

/** The zoom the settings hold, as one of the steps. */
export function currentZoom(): number {
  const stored = useSettingsStore.getState().values[ZOOM_SETTING_KEY];
  return typeof stored === "number" ? snapZoom(stored) : DEFAULT_ZOOM;
}

/** Zooms in, out or back to 100%: changes the setting, which the shell applies at once. */
export function stepAppZoom(action: ZoomAction): void {
  const current = currentZoom();
  const next = nextZoom(current, action);
  if (next !== current) useSettingsStore.getState().set(ZOOM_SETTING_KEY, next);
}

/**
 * Zooms the whole webview (layout, text and images, like a program's own zoom). In a plain browser
 * (the synthetic backend) there is no webview to ask and nothing happens.
 */
export async function applyNativeZoom(percent: number): Promise<void> {
  if (!isTauri()) return;
  try {
    await getCurrentWebview().setZoom(snapZoom(percent) / 100);
  } catch {
    // A webview that refuses only costs the zoom; the setting stays.
  }
}

/** Puts the zoom of the last session back at once, before the settings file has been read. */
export function applyBootZoom(): void {
  const zoom = readBootLook()?.zoom;
  if (zoom !== undefined && snapZoom(zoom) !== DEFAULT_ZOOM) void applyNativeZoom(zoom);
}
