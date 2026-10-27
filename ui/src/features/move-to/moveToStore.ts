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
}

export interface MoveToRequest {
  ids: readonly string[];
  /** The titles by name for toasts: "Heat (1995)" or "4 titles". */
  label: string;
  kind: MoveKind;
  anchor: MoveAnchor;
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
  open: (request: MoveToRequest) => void;
  close: () => void;
  setRunning: (running: RunningMove | null) => void;
}

export const useMoveToStore = create<MoveToState>((set) => ({
  request: null,
  running: null,
  open: (request) => set({ request }),
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

function anchorOf(button: HTMLButtonElement): MoveAnchor {
  const rect = button.getBoundingClientRect();
  return {
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
    side: button.closest('[role="toolbar"]') === null ? "bottom" : "top",
  };
}

/** How long a press counts as the reason for a request (a menu item chosen right after it). */
const PRESS_MS = 1500;

/**
 * The anchor of a request made right now: the button that was pressed or has the keyboard focus,
 * else the pointer's last place, else the middle of the window.
 */
export function currentAnchor(): MoveAnchor {
  const active = document.activeElement;
  if (active instanceof HTMLButtonElement) return anchorOf(active);
  const recent = pressed !== null && Date.now() - pressed.at < PRESS_MS ? pressed : null;
  if (recent?.button?.isConnected === true) return anchorOf(recent.button);
  const at = recent ?? { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  return { x: at.x, y: at.y, width: 1, height: 1, side: "bottom" };
}

/** Opens the Move to menu for these titles (the Library, the Drives page and the title pages). */
export function openMoveTo(ids: readonly string[], kind: MoveKind, label: string): void {
  if (ids.length === 0) return;
  useMoveToStore.getState().open({ ids: [...ids], label, kind, anchor: currentAnchor() });
}
