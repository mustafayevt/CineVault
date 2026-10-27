import { create } from "zustand";

import { SIDEBAR_SETTING_KEY, useSettingsStore } from "../features/settings/settingsStore";

export type Theme = "dark" | "light";

/** A choice made with the toggle; it lasts until the window crosses the breakpoint. */
export type SidebarOverride = "expanded" | "collapsed";

interface ShellState {
  /** The theme that is drawn. The Theme setting owns the choice; System resolves to one of two. */
  theme: Theme;
  /** Saves the choice as the Theme setting. */
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;
  /** Whether the command palette is open (rendered by the palette). */
  paletteOpen: boolean;
  openPalette: () => void;
  closePalette: () => void;
  /** Whether the window is narrower than {@link SIDEBAR_RAIL_BREAKPOINT} (kept by the shell). */
  narrowWindow: boolean;
  /** Records the window width class; crossing the breakpoint drops a manual sidebar choice. */
  setNarrowWindow: (narrow: boolean) => void;
  sidebarOverride: SidebarOverride | null;
  /** Flips the sidebar between full and rail, whatever the window width. */
  toggleSidebar: () => void;
  /** Whether the window is narrower than {@link SIDEBAR_HIDE_BREAKPOINT} (kept by the shell). */
  companionWindow: boolean;
  setCompanionWindow: (companion: boolean) => void;
  /** The navigation sheet of the companion layout is open over the page. */
  navSheetOpen: boolean;
  setNavSheetOpen: (open: boolean) => void;
}

/** True when the sidebar shows as the icon rail: a manual choice wins, else the window width. */
export function selectSidebarCollapsed(state: ShellState): boolean {
  return state.sidebarOverride === null
    ? state.narrowWindow
    : state.sidebarOverride === "collapsed";
}

/** Remembers the sidebar choice across launches; `null` forgets it. */
function persistSidebar(override: SidebarOverride | null): void {
  const settings = useSettingsStore.getState();
  if (override === null) settings.reset([SIDEBAR_SETTING_KEY]);
  else settings.set(SIDEBAR_SETTING_KEY, override);
}

export const useShellStore = create<ShellState>((set, get) => ({
  theme: "dark",
  setTheme: (theme) => {
    set({ theme });
    useSettingsStore.getState().set("appearance.theme", theme);
  },
  toggleTheme: () => get().setTheme(get().theme === "dark" ? "light" : "dark"),
  paletteOpen: false,
  openPalette: () => set({ paletteOpen: true }),
  closePalette: () => set({ paletteOpen: false }),
  narrowWindow: false,
  setNarrowWindow: (narrow) => {
    const state = get();
    if (state.narrowWindow === narrow) return;
    set({ narrowWindow: narrow, sidebarOverride: null });
    // Crossing the breakpoint drops a manual choice. Before the stored settings are read the
    // saved choice must survive, because the window's first measurement is not a crossing.
    if (state.sidebarOverride !== null) persistSidebar(null);
  },
  companionWindow: false,
  setCompanionWindow: (companion) => {
    if (get().companionWindow === companion) return;
    set({ companionWindow: companion, navSheetOpen: false });
  },
  navSheetOpen: false,
  setNavSheetOpen: (open) => set({ navSheetOpen: open }),
  sidebarOverride: null,
  toggleSidebar: () => {
    const override = selectSidebarCollapsed(get()) ? "expanded" : "collapsed";
    set({ sidebarOverride: override });
    persistSidebar(override);
  },
}));
