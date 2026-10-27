import { useEffect, useState } from "react";

import { useShellStore } from "../../stores/shell";
import type { Theme } from "../../stores/shell";
import { SIDEBAR_SETTING_KEY, useSetting, useSettingsStore } from "./settingsStore";
import { readBootLook, saveBootLook } from "./bootLook";
import { DEFAULT_ZOOM, applyNativeZoom, snapZoom } from "./zoom";

export type ThemePreference = Theme | "system";
export type MotionPreference = "system" | "reduced" | "full";

const LIGHT_QUERY = "(prefers-color-scheme: light)";
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/** What `data-motion` holds on the root: "reduced" cuts every transition to a short crossfade. */
export type MotionAttribute = "reduced" | "full";

/**
 * The attribute value for a Motion choice. Reduced is set by the choice itself or, for System, by the
 * operating system's reduced-motion preference; Full forces motion on over that preference.
 */
export function resolveMotion(
  preference: MotionPreference,
  systemPrefersReduced: boolean,
): MotionAttribute | null {
  if (preference === "reduced") return "reduced";
  if (preference === "full") return "full";
  return systemPrefersReduced ? "reduced" : null;
}

/** The theme to draw: an explicit choice, or the operating system's when the choice is System. */
function resolveTheme(preference: ThemePreference, systemPrefersLight: boolean): Theme {
  if (preference === "system") return systemPrefersLight ? "light" : "dark";
  return preference;
}

function media(query: string): MediaQueryList | null {
  return typeof window.matchMedia === "function" ? window.matchMedia(query) : null;
}

/** Writes the look onto the document so it changes at once, with no restart. */
export function applyAppearance(
  root: HTMLElement,
  look: {
    theme: Theme;
    motion?: MotionPreference;
    /** The operating system's prefers-reduced-motion, used when `motion` is "system". */
    systemReducedMotion?: boolean;
  },
) {
  root.dataset.theme = look.theme;
  const motion = resolveMotion(look.motion ?? "system", look.systemReducedMotion ?? false);
  if (motion === null) delete root.dataset.motion;
  else root.dataset.motion = motion;
}

/**
 * Applies the Appearance settings to the window as they change: theme (following the operating
 * system when asked to), motion and the zoom. The shell store's `theme` follows, so the palette
 * and other readers see the theme that is drawn.
 */
export function useAppearance(): void {
  const stored = {
    theme: useSetting<ThemePreference>("appearance.theme"),
    motion: useSetting<MotionPreference>("appearance.motion"),
    zoom: useSetting<number>("appearance.zoom"),
  };
  // Until the settings file is read, the look of the last session stands in for the defaults,
  // so the window does not flash the default theme on every start.
  const ready = useSettingsStore((state) => state.status === "ready");
  const [boot] = useState(readBootLook);
  const {
    theme: preference,
    motion,
    zoom,
  } = ready || boot === null ? stored : { ...boot, zoom: boot.zoom ?? DEFAULT_ZOOM };

  useEffect(() => {
    if (ready) saveBootLook({ theme: preference, motion, zoom });
  }, [ready, preference, motion, zoom]);

  // The whole webview zooms, so layout, text and images scale like a program's own zoom.
  useEffect(() => {
    void applyNativeZoom(snapZoom(zoom));
  }, [zoom]);

  useEffect(() => {
    const light = media(LIGHT_QUERY);
    const reduced = media(REDUCED_MOTION_QUERY);
    const apply = () => {
      const theme = resolveTheme(preference, light?.matches ?? false);
      applyAppearance(document.documentElement, {
        theme,
        motion,
        systemReducedMotion: reduced?.matches ?? false,
      });
      useShellStore.setState({ theme });
    };
    apply();
    const watched = [preference === "system" ? light : null, motion === "system" ? reduced : null];
    const active = watched.filter((query): query is MediaQueryList => query !== null);
    for (const query of active) query.addEventListener("change", apply);
    return () => {
      for (const query of active) query.removeEventListener("change", apply);
    };
  }, [preference, motion]);
}

/** Loads the stored settings once and puts the saved sidebar choice back. */
export function useSettingsLoader(): void {
  const load = useSettingsStore((state) => state.load);
  useEffect(() => {
    void load().then(() => {
      const saved = useSettingsStore.getState().values[SIDEBAR_SETTING_KEY];
      if (saved === "expanded" || saved === "collapsed") {
        useShellStore.setState({ sidebarOverride: saved });
      }
    });
  }, [load]);
}
