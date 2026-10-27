import { useCallback, useMemo, useState } from "react";
import type { KeyboardEvent, MouseEvent } from "react";

/** A list item's identity: a title id, a path, a row number. */
export type ListKey = string | number;

export interface ListSelectionOptions<K extends ListKey> {
  /** Every item of the list in display order. */
  keys: readonly K[];
  /** Opens an item: Enter on the current item and double-click. Left out, lists have no Open. */
  onOpen?: (key: K) => void;
  /** Items per row in a grid: Left/Right step one item, Up/Down one row. A list leaves it at 1. */
  columns?: number;
  /** Items PageUp and PageDown move; 10 by default. */
  pageSize?: number;
}

/** Modifier keys of a click, so a pointer handler and a test can both pass them. */
export interface ClickModifiers {
  /** Ctrl on Windows, Cmd on macOS: add or remove one item. */
  toggle?: boolean;
  /** Shift: select the range from the anchor. */
  range?: boolean;
}

export interface ListSelection<K extends ListKey> {
  /** The selected items that are in the list now, in no particular order. */
  selected: ReadonlySet<K>;
  count: number;
  /** The item the keyboard is on (the last one clicked or moved to), or null. */
  current: K | null;
  /** The index of `current` in `keys`, or -1. */
  currentIndex: number;
  isSelected: (key: K) => boolean;
  /** A click on an item with its modifiers. */
  click: (key: K, modifiers?: ClickModifiers) => void;
  /** Right-click or Shift+F10 on an item: selects it alone unless it is already selected. */
  target: (key: K) => void;
  open: (key: K) => void;
  selectAll: () => void;
  clear: () => void;
  /** Replaces the selection (the current item becomes the last of them). */
  select: (keys: Iterable<K>) => void;
  /** Handles a key of the list; returns true when the key did something (callers prevent default). */
  onKeyDown: (event: KeyboardEvent | globalThis.KeyboardEvent) => boolean;
  /** Pointer props for one item: click with Ctrl/Cmd and Shift, double-click to open. */
  itemProps: (key: K) => {
    onClick: (event: MouseEvent) => void;
    onDoubleClick: (event: MouseEvent) => void;
  };
}

interface State<K> {
  selected: ReadonlySet<K>;
  current: K | null;
  anchor: K | null;
}

const EMPTY: State<never> = { selected: new Set(), current: null, anchor: null };

function rangeOf<K>(keys: readonly K[], from: number, to: number): K[] {
  const [start, end] = from <= to ? [from, to] : [to, from];
  return keys.slice(start, end + 1);
}

/**
 * Desktop multi-select for every grid and table (spec desktop-interaction "Desktop
 * multi-select"): a click selects one item, Ctrl/⌘-click adds or removes one, Shift-click selects
 * the range from the anchor (Ctrl+Shift adds it), Ctrl/⌘+A selects all, arrows move the current
 * item (Shift extends from the anchor, Ctrl moves without selecting, Space toggles), Home/End and
 * PageUp/PageDown jump, Enter and double-click open, Esc clears. Items that leave the list (a
 * filter) drop out of the selection. Pass `keys` memoised: a new array is a new list.
 */
export function useListSelection<K extends ListKey>({
  keys,
  onOpen,
  columns = 1,
  pageSize = 10,
}: ListSelectionOptions<K>): ListSelection<K> {
  const [state, setState] = useState<State<K>>(EMPTY);

  const indexOf = useMemo(() => {
    const map = new Map<K, number>();
    keys.forEach((key, index) => map.set(key, index));
    return map;
  }, [keys]);

  // Items that leave the list (a filter, a removal) leave the selection for good, so a later
  // action never reaches titles the user can no longer see.
  const [listed, setListed] = useState(indexOf);
  if (listed !== indexOf) {
    setListed(indexOf);
    const gone = (key: K | null) => key !== null && !indexOf.has(key);
    if ([...state.selected].some(gone) || gone(state.current) || gone(state.anchor)) {
      setState({
        selected: new Set([...state.selected].filter((key) => indexOf.has(key))),
        current: gone(state.current) ? null : state.current,
        anchor: gone(state.anchor) ? null : state.anchor,
      });
    }
  }
  const { selected, current, anchor } = state;
  const currentIndex = current === null ? -1 : (indexOf.get(current) ?? -1);

  const click = useCallback(
    (key: K, modifiers: ClickModifiers = {}) => {
      const at = indexOf.get(key);
      if (at === undefined) return;
      if (modifiers.range === true) {
        const from = anchor === null ? at : (indexOf.get(anchor) ?? at);
        const range = rangeOf(keys, from, at);
        const next = modifiers.toggle === true ? new Set([...selected, ...range]) : new Set(range);
        setState({ selected: next, current: key, anchor: anchor ?? key });
      } else if (modifiers.toggle === true) {
        const next = new Set(selected);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        setState({ selected: next, current: key, anchor: key });
      } else {
        setState({ selected: new Set([key]), current: key, anchor: key });
      }
    },
    [indexOf, keys, anchor, selected],
  );

  const target = useCallback(
    (key: K) => {
      if (!indexOf.has(key)) return;
      if (selected.has(key)) setState({ selected, current: key, anchor: anchor ?? key });
      else setState({ selected: new Set([key]), current: key, anchor: key });
    },
    [indexOf, selected, anchor],
  );

  const open = useCallback((key: K) => onOpen?.(key), [onOpen]);
  const selectAll = useCallback(() => setState((s) => ({ ...s, selected: new Set(keys) })), [keys]);
  const clear = useCallback(() => setState((s) => ({ ...s, selected: new Set() })), []);
  const select = useCallback((items: Iterable<K>) => {
    const next = new Set(items);
    const last = [...next].at(-1) ?? null;
    setState({ selected: next, current: last, anchor: last });
  }, []);

  const onKeyDown = useCallback(
    (event: KeyboardEvent | globalThis.KeyboardEvent): boolean => {
      const toggle = event.ctrlKey || event.metaKey;
      if (toggle && !event.altKey && (event.key === "a" || event.key === "A")) {
        selectAll();
        return true;
      }
      if (event.key === "Escape") {
        if (selected.size === 0) return false;
        clear();
        return true;
      }
      if (event.key === "Enter") {
        if (current === null || onOpen === undefined) return false;
        onOpen(current);
        return true;
      }
      if (event.key === " ") {
        if (current === null) return false;
        click(current, { toggle: true });
        return true;
      }
      if (keys.length === 0) return false;
      const last = keys.length - 1;
      const step = (() => {
        switch (event.key) {
          case "ArrowDown":
            return columns;
          case "ArrowUp":
            return -columns;
          case "ArrowRight":
            return columns > 1 ? 1 : null;
          case "ArrowLeft":
            return columns > 1 ? -1 : null;
          case "PageDown":
            return pageSize * columns;
          case "PageUp":
            return -pageSize * columns;
          case "Home":
            return -Infinity;
          case "End":
            return Infinity;
          default:
            return null;
        }
      })();
      if (step === null) return false;
      const base = currentIndex === -1 ? (step > 0 ? -1 : keys.length) : currentIndex;
      const raw = Number.isFinite(step) ? base + step : step > 0 ? last : 0;
      const nextIndex = Math.min(last, Math.max(0, raw));
      const nextKey = keys[nextIndex];
      if (nextKey === undefined) return false;
      if (event.shiftKey) {
        const from = anchor === null ? nextIndex : (indexOf.get(anchor) ?? nextIndex);
        setState({
          selected: new Set(rangeOf(keys, from, nextIndex)),
          current: nextKey,
          anchor: anchor ?? nextKey,
        });
      } else if (toggle) {
        setState({ selected, current: nextKey, anchor: nextKey });
      } else {
        setState({ selected: new Set([nextKey]), current: nextKey, anchor: nextKey });
      }
      return true;
    },
    [
      keys,
      indexOf,
      selected,
      current,
      currentIndex,
      anchor,
      columns,
      pageSize,
      onOpen,
      click,
      selectAll,
      clear,
    ],
  );

  const itemProps = useCallback(
    (key: K) => ({
      onClick: (event: MouseEvent) =>
        click(key, { toggle: event.ctrlKey || event.metaKey, range: event.shiftKey }),
      onDoubleClick: () => onOpen?.(key),
    }),
    [click, onOpen],
  );

  const isSelected = useCallback((key: K) => selected.has(key), [selected]);

  return {
    selected,
    count: selected.size,
    current,
    currentIndex,
    isSelected,
    click,
    target,
    open,
    selectAll,
    clear,
    select,
    onKeyDown,
    itemProps,
  };
}
