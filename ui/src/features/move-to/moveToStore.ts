import { create } from "zustand";

/** The kinds of title a move can carry; all titles of one request are of the same kind. */
export type MoveKind = "movie" | "show";

/** Where the Move to menu opens: beside the control that asked, or at the pointer. */
export interface MoveAnchor {
  x: number;
  y: number;
  width: number;
  height: number;
  /** A control in the floating selection bar opens the menu upwards. */
  side: "top" | "bottom";
  /**
   * The control that asked, when there is one: the menu follows it while it moves (the selection
   * bar slides in) and pressing it again closes the menu instead of opening it once more.
   */
  element?: HTMLElement | null;
}

export interface MoveToRequest {
  /** Tells one request from the next, so a new request is a fresh menu. */
  id: number;
  ids: readonly string[];
  /** The titles by name for toasts: "Heat (1995)" or "4 titles". */
  label: string;
  kind: MoveKind;
  anchor: MoveAnchor;
  /** Asked from the keyboard (no press just before): the first destination takes the focus. */
  keyboard: boolean;
}

/** A move that is running, for the sidebar's progress card and the Drives page's progress line. */
export interface RunningMove {
  /** What is moving: "Heat (1995)" or "3 titles". */
  label: string;
  /** The destination's name. */
  target: string;
  percent: number;
  /** The title being moved right now, when the backend reported one. */
  current?: string;
  /** "142 MB/s · 4 min left" while files are copied; absent until it is known. */
  speed?: string;
  paused: boolean;
}

interface MoveToState {
  request: MoveToRequest | null;
  running: RunningMove | null;
  open: (request: Omit<MoveToRequest, "id">) => void;
  close: () => void;
  setRunning: (running: RunningMove | null) => void;
}

let requests = 0;

export const useMoveToStore = create<MoveToState>((set) => ({
  request: null,
  running: null,
  open: (request) => set({ request: { ...request, id: ++requests } }),
  close: () => set({ request: null }),
  setRunning: (running) => set({ running }),
}));

/** Where the pointer last went down, and the button under it (a click does not focus one in every webview). */
let pressed: { x: number; y: number; button: HTMLButtonElement | null; at: number } | null = null;

/** Remembers where the pointer last went down, so a menu item can open the menu beside it. */
export function trackPointer(event: {
  clientX: number;
  clientY: number;
  target: EventTarget | null;
}): void {
  const target = event.target instanceof Element ? event.target : null;
  pressed = {
    x: event.clientX,
    y: event.clientY,
    button: target?.closest("button") ?? null,
    at: Date.now(),
  };
}

/**
 * The box the menu lines up with: the control itself, or for a control in a toolbar (the selection
 * bar) the control's width and the toolbar's height, so a menu that opens upwards clears the bar.
 */
function boxOf(element: HTMLElement): Pick<MoveAnchor, "x" | "y" | "width" | "height"> {
  const rect = element.getBoundingClientRect();
  const bar = element.closest('[role="toolbar"]')?.getBoundingClientRect();
  if (bar === undefined || bar.height === 0) {
    return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
  }
  return { x: rect.left, y: bar.top, width: rect.width, height: bar.height };
}

function anchorOf(button: HTMLButtonElement): MoveAnchor {
  return {
    ...boxOf(button),
    side: button.closest('[role="toolbar"]') === null ? "bottom" : "top",
    element: button,
  };
}

/** Where an anchor's control is now, or the anchor as it was taken when the control is gone. */
export function liveRect(anchor: MoveAnchor): Pick<MoveAnchor, "x" | "y" | "width" | "height"> {
  const element = anchor.element;
  if (element?.isConnected !== true) return anchor;
  const box = boxOf(element);
  return box.width === 0 && box.height === 0 ? anchor : box;
}

/** How long a press counts as the reason for a request (a menu item chosen right after it). */
const PRESS_MS = 1500;

/**
 * The anchor of a request made right now: the button that was just pressed, else the one with the
 * keyboard focus, else the pointer's last place, else the middle of the window. The press comes
 * first because a click does not focus a button in every webview, so the focus can still be on a
 * button pressed earlier.
 */
export function currentAnchor(): MoveAnchor {
  const recent = pressed !== null && Date.now() - pressed.at < PRESS_MS ? pressed : null;
  if (recent?.button?.isConnected === true) return anchorOf(recent.button);
  const active = document.activeElement;
  if (active instanceof HTMLButtonElement) return anchorOf(active);
  const at = recent ?? { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  return { x: at.x, y: at.y, width: 1, height: 1, side: "bottom" };
}

/** Opens the Move to menu for these titles (the Library, the Drives page and the title pages). */
export function openMoveTo(ids: readonly string[], kind: MoveKind, label: string): void {
  if (ids.length === 0) return;
  const store = useMoveToStore.getState();
  const anchor = currentAnchor();
  // The control that opened the menu closes it again, like any other menu button.
  const opener = store.request?.anchor.element;
  if (opener !== undefined && opener !== null && opener === anchor.element) {
    store.close();
    return;
  }
  // A request with no press just before it, or a press on something other than the control that
  // asks, came from the keyboard.
  const recent = pressed !== null && Date.now() - pressed.at < PRESS_MS ? pressed : null;
  const keyboard =
    recent === null ||
    (anchor.element !== undefined && anchor.element !== null && recent.button !== anchor.element);
  store.open({ ids: [...ids], label, kind, anchor, keyboard });
}
