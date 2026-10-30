import { useEffect, useRef } from "react";

import { transport } from "../../lib/transport";
import { stepAppZoom, zoomActionForKey } from "../settings/zoom";
import type { ZoomAction } from "../settings/zoom";
import { handleEscape } from "./escStack";
import { detectPlatform } from "./platform";

/** The attribute that marks the title-bar library search, the target of Ctrl+F. */
export const LIBRARY_SEARCH_ATTRIBUTE = "data-library-search";

/** An element that may be dragged out of the window on purpose (artwork tiles, files). */
export const DRAG_SOURCE_ATTRIBUTE = "data-drag-source";

export interface DesktopGuardOptions {
  /** Ctrl+F: focus the library search. Defaults to the element marked {@link LIBRARY_SEARCH_ATTRIBUTE}. */
  focusSearch?: () => void;
  /** The keyboard's own browser-back key. */
  back?: () => void;
  /** The keyboard's own browser-forward key. */
  forward?: () => void;
  /** Ctrl/⌘ plus, minus and 0: the app zoom (the browser's own zoom stays off). */
  zoom?: (action: ZoomAction) => void;
  /** Whether the zoom modifier is Command (macOS) and not Ctrl. Defaults to the platform. */
  mac?: boolean;
  /** Also blocks the developer-tool keys (release builds). */
  blockDevTools?: boolean;
}

/** Puts focus in the title-bar library search and selects its text. */
function focusLibrarySearch(root: ParentNode = document): boolean {
  const field = root.querySelector<HTMLInputElement>(`[${LIBRARY_SEARCH_ATTRIBUTE}]`);
  if (field === null) {
    return false;
  }
  field.focus();
  field.select();
  return true;
}

const BROWSER_KEYS = new Set([
  "BrowserRefresh",
  "BrowserSearch",
  "BrowserHome",
  "BrowserStop",
  "BrowserFavorites",
]);

/** What a key press does in the browser that CineVault turns off, or `null` when nothing. */
type Blocked = "block" | "search" | "back" | "forward" | ZoomAction;

/**
 * Classifies a key press. Pure, so a test can fire every blocked key. Alt+Left and Alt+Right are
 * CineVault's own back and forward and are handled by the sidebar, not here.
 */
function classifyKey(
  event: Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">,
  blockDevTools: boolean,
  mac: boolean,
): Blocked | null {
  const { key } = event;
  if (key === "BrowserBack") return "back";
  if (key === "BrowserForward") return "forward";
  if (BROWSER_KEYS.has(key)) return "block";
  if (key === "F5" || key === "F3") return "block";
  if (blockDevTools) {
    if (key === "F12") return "block";
    const lower = key.toLowerCase();
    if ((event.ctrlKey || event.metaKey) && event.shiftKey && ["i", "j", "c"].includes(lower)) {
      return "block";
    }
    if (event.metaKey && event.altKey && ["i", "j", "c"].includes(lower)) return "block";
  }
  const mod = event.ctrlKey || event.metaKey;
  if (!mod || event.altKey) return null;
  const lower = key.toLowerCase();
  if (lower === "f" && !event.shiftKey) return "search";
  if (["r", "p", "u", "g"].includes(lower)) return "block";
  // Zoom: plus (the plus key and the equals key it shares), minus, zero. The platform's own
  // modifier drives the app zoom; the other chord does nothing.
  const zoom = zoomActionForKey(event, mac);
  if (zoom !== null) return zoom;
  if (["+", "=", "-", "_", "0"].includes(key)) return "block";
  return null;
}

/**
 * Installs the guards that make the window behave like a program and not a web page:
 * the browser's reload, print, view-source and find keys do nothing of their own, Ctrl+F
 * focuses the library search, Ctrl plus, minus and 0 zoom the app in steps (the browser's zoom
 * stays off), Ctrl+wheel and pinch do not zoom, the browser context menu never
 * shows, and images and links do not start a ghost drag. The Esc stack is driven from here too.
 *
 * Every listener runs in the bubble phase on the window, after the page's own handlers. In
 * particular `contextmenu` is not cancelled on the way down, so a `ContextMenu` still opens on its
 * own targets; only the browser's menu is suppressed, where nobody else handled the event.
 * Returns the function that removes them.
 */
export function installDesktopGuards(options: DesktopGuardOptions = {}): () => void {
  const mac = options.mac ?? detectPlatform() === "macos";
  const browser = transport() === "browser";
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      if (handleEscape(event)) {
        event.preventDefault();
      }
      return;
    }
    // A browser keeps its reload, zoom, find and developer tools. CineVault only takes Ctrl/Cmd+F
    // for the library search.
    if (browser) {
      const modifier = mac ? event.metaKey : event.ctrlKey;
      if (modifier && !event.altKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        if (options.focusSearch !== undefined) options.focusSearch();
        else focusLibrarySearch();
      }
      return;
    }
    const action = classifyKey(event, options.blockDevTools ?? false, mac);
    if (action === null) return;
    event.preventDefault();
    if (action === "search") {
      if (options.focusSearch !== undefined) options.focusSearch();
      else focusLibrarySearch();
    } else if (action === "back") {
      options.back?.();
    } else if (action === "forward") {
      options.forward?.();
    } else if (action === "in" || action === "out" || action === "reset") {
      (options.zoom ?? stepAppZoom)(action);
    }
  };

  const onContextMenu = (event: MouseEvent) => {
    // Our own menus open in their handlers, which ran before this one. A browser keeps its menu
    // where the app has none.
    if (browser) return;
    event.preventDefault();
  };

  const onDragStart = (event: DragEvent) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const draggable = target.closest("img, a");
    if (draggable === null) return;
    if (draggable.closest(`[${DRAG_SOURCE_ATTRIBUTE}]`) !== null) return;
    event.preventDefault();
  };

  const onWheel = (event: WheelEvent) => {
    if (browser) return;
    if (event.ctrlKey || event.metaKey) event.preventDefault();
  };

  const onGesture = (event: Event) => {
    if (!browser) event.preventDefault();
  };

  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("contextmenu", onContextMenu);
  window.addEventListener("dragstart", onDragStart);
  window.addEventListener("wheel", onWheel, { passive: false });
  window.addEventListener("gesturestart", onGesture);
  window.addEventListener("gesturechange", onGesture);
  return () => {
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("contextmenu", onContextMenu);
    window.removeEventListener("dragstart", onDragStart);
    window.removeEventListener("wheel", onWheel);
    window.removeEventListener("gesturestart", onGesture);
    window.removeEventListener("gesturechange", onGesture);
  };
}

/** Installs the guards for the life of the shell. */
export function useDesktopGuards(options: DesktopGuardOptions = {}): void {
  const latest = useRef(options);
  useEffect(() => {
    latest.current = options;
  });
  useEffect(
    () =>
      installDesktopGuards({
        focusSearch: () => {
          if (latest.current.focusSearch !== undefined) latest.current.focusSearch();
          else focusLibrarySearch();
        },
        zoom: (action) => (latest.current.zoom ?? stepAppZoom)(action),
        back: () => latest.current.back?.(),
        forward: () => latest.current.forward?.(),
        blockDevTools: !import.meta.env.DEV,
      }),
    [],
  );
}
