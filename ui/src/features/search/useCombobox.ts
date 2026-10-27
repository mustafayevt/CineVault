import { useEffect, useId, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";

interface ComboboxOptions<E extends HTMLElement> {
  /** The element the panel hangs under (the field's wrapper). */
  anchor: E | null;
  /** Whether the panel has anything to show (rows, or a message); Esc only closes a panel that shows. */
  showing?: boolean;
  /** The ids of the rows in order: what the arrow keys walk through. */
  ids: readonly string[];
  /** A row was chosen with Enter or a click. */
  onSelect: (id: string) => void;
}

/** Everything a field needs to be the combobox of a `SuggestPanel`. */
export interface Combobox<E extends HTMLElement = HTMLElement> {
  open: boolean;
  setOpen: (open: boolean) => void;
  activeId: string | null;
  setActive: (id: string | null) => void;
  listboxId: string;
  optionId: (id: string) => string;
  /** Set on the panel element: a callback ref. */
  attachPanel: (element: HTMLDivElement | null) => void;
  /** The two elements once mounted. */
  anchor: E | null;
  panel: HTMLDivElement | null;
  select: (id: string) => void;
  /** Props for the field itself. */
  fieldProps: {
    role: "combobox";
    "aria-expanded": boolean;
    "aria-controls": string;
    "aria-autocomplete": "list";
    "aria-haspopup": "listbox";
    "aria-activedescendant": string | undefined;
    onKeyDown: (event: ReactKeyboardEvent) => void;
    onFocus: () => void;
    onClick: () => void;
  };
  /** The text changed: show the panel again and forget the highlight. */
  typed: () => void;
}

/**
 * The state and keys of a combobox over a list of row ids. Up and Down move the highlight and
 * wrap, Enter chooses the highlighted row (with none, the key goes on to the form), Esc closes the
 * panel before it does anything else, Tab leaves, and a press outside the field and the panel
 * closes it.
 */
export function useCombobox<E extends HTMLElement = HTMLElement>({
  anchor,
  ids,
  onSelect,
  showing,
}: ComboboxOptions<E>): Combobox<E> {
  const listboxId = useId();
  const [panel, setPanel] = useState<HTMLDivElement | null>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const activeId = active !== null && ids.includes(active) ? active : null;
  const optionId = (id: string) => `${listboxId}-${id.replace(/[^A-Za-z0-9_-]/g, "_")}`;

  useEffect(() => {
    if (!open) return;
    const onPress = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (anchor?.contains(target) === true) return;
      if (panel?.contains(target) === true) return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", onPress, true);
    return () => document.removeEventListener("pointerdown", onPress, true);
  }, [open, anchor, panel]);

  const select = (id: string) => {
    setOpen(false);
    setActive(null);
    onSelect(id);
  };

  const onKeyDown = (event: ReactKeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      if (ids.length === 0) return;
      event.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      const at = activeId === null ? -1 : ids.indexOf(activeId);
      const next =
        event.key === "ArrowDown" ? (at + 1) % ids.length : at <= 0 ? ids.length - 1 : at - 1;
      setActive(ids[next] ?? null);
    } else if (event.key === "Enter") {
      if (open && activeId !== null) {
        event.preventDefault();
        select(activeId);
      }
    } else if (event.key === "Escape") {
      if (open && (showing ?? ids.length > 0)) {
        // The first Esc only closes the list; the field keeps its text until the next one.
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
        setActive(null);
      }
    } else if (event.key === "Tab") {
      setOpen(false);
    }
  };

  return {
    open,
    setOpen,
    activeId,
    setActive,
    listboxId,
    optionId,
    attachPanel: setPanel,
    anchor,
    panel,
    select,
    fieldProps: {
      role: "combobox",
      "aria-expanded": open && (showing ?? ids.length > 0),
      "aria-controls": listboxId,
      "aria-autocomplete": "list",
      "aria-haspopup": "listbox",
      "aria-activedescendant": open && activeId !== null ? optionId(activeId) : undefined,
      onKeyDown,
      onFocus: () => setOpen(true),
      onClick: () => setOpen(true),
    },
    typed: () => {
      setOpen(true);
      setActive(null);
    },
  };
}
