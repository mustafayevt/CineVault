import { useEffect, useRef } from "react";

/**
 * The global Esc stack (spec `desktop-interaction` "Esc closes the innermost thing"). Esc closes,
 * in order: an open menu or tooltip, an open dialog, an open picker page, and last the current
 * selection. Radix menus, popovers, tooltips and dialogs already dismiss themselves (and mark the
 * key as handled); pickers, the selection and any other layer register here, so one press closes
 * exactly one thing and the rest wait for the next press.
 */
export type EscapeLayer = "menu" | "dialog" | "picker" | "selection";

/** Highest first: the layer that sits on top of the others closes first. */
const RANK: Readonly<Record<EscapeLayer, number>> = {
  menu: 3,
  dialog: 2,
  picker: 1,
  selection: 0,
};

interface Entry {
  layer: EscapeLayer;
  /** Closes the layer. Returning `false` means it was not open after all; the next one gets the key. */
  close: () => unknown;
}

const entries: Entry[] = [];

/** Adds a layer to the stack; returns the function that removes it. */
export function pushEscape(layer: EscapeLayer, close: Entry["close"]): () => void {
  const entry: Entry = { layer, close };
  entries.push(entry);
  return () => {
    const index = entries.indexOf(entry);
    if (index >= 0) {
      entries.splice(index, 1);
    }
  };
}

/** Registers a layer while `active`. The handler may change between renders. */
export function useEscapeLayer(layer: EscapeLayer, active: boolean, close: Entry["close"]): void {
  const latest = useRef(close);
  useEffect(() => {
    latest.current = close;
  });
  useEffect(() => {
    if (!active) {
      return;
    }
    return pushEscape(layer, () => latest.current());
  }, [layer, active]);
}

/** How many layers are open (for tests and the selection bar's own checks). */
export function escapeDepth(): number {
  return entries.length;
}

function typedText(target: EventTarget | null): boolean {
  return (
    (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) &&
    target.value !== "" &&
    !["checkbox", "radio", "button", "range"].includes(target.type)
  );
}

/**
 * Closes the innermost layer for one Esc press and reports whether something closed. Later
 * registrations of the same layer close first. A key already handled (a Radix layer dismissed
 * itself) closes nothing more, and Esc never discards typed text: a field that holds text keeps
 * the key for itself (the search field clears first; a plain field is left alone).
 */
export function handleEscape(event: Pick<KeyboardEvent, "defaultPrevented" | "target">): boolean {
  if (event.defaultPrevented || typedText(event.target)) {
    return false;
  }
  const ordered = entries
    .map((entry, index) => ({ entry, index }))
    .sort((a, b) => RANK[b.entry.layer] - RANK[a.entry.layer] || b.index - a.index);
  for (const { entry } of ordered) {
    if (entry.close() !== false) {
      return true;
    }
  }
  return false;
}
