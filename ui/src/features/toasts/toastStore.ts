import type { LucideIcon } from "lucide-react";
import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";

/** Reverses the action a toast confirmed. Throwing reports the failure to the notification centre. */
export type UndoHandler = () => void | Promise<void>;

export interface ToastInput {
  /** A past-tense statement with a count: "Marked 12 watched". */
  message: string;
  /** The glyph of the action the toast confirms, drawn before the message. */
  glyph?: LucideIcon;
  /** Makes the toast offer Undo. Without it the toast only confirms. */
  undo?: UndoHandler;
  /** Visible time in ms; `null` keeps the toast until dismissed. */
  duration?: number | null;
  /** An error that needs action: styled as one, with a link to Activity where the details are. */
  tone?: "default" | "error";
  /**
   * A text button after the message that does something about it ("Copy magnet link"). On an
   * error toast it replaces the link to Activity.
   */
  action?: { label: string; onClick: () => void };
}

export interface ToastEntry extends ToastInput {
  /** Doubles as the undo token: undoing is addressed by this id and works once. */
  id: string;
  /** How many identical messages this toast stands for ("x3"); 1 for a single one. */
  count: number;
}

/** At most this many toasts show; a newer one pushes the oldest out. */
export const MAX_TOASTS = 3;

/**
 * How many past actions stay undoable after their toast has gone. Ctrl Z walks back through them,
 * newest first, so a file move can still be taken back once its toast has timed out.
 */
const UNDO_HISTORY = 20;

interface ToastState {
  /** Oldest first, so the newest sits last in the stack. */
  toasts: readonly ToastEntry[];
  /** The actions that can still be undone, oldest first, whether their toast shows or not. */
  history: readonly ToastEntry[];
  push: (input: ToastInput) => string;
  /** Hides a toast. Its action stays undoable with the shortcut. */
  dismiss: (id: string) => void;
  /**
   * Runs the action's undo once and removes its toast. Says "Undone: ..." when the undo did not
   * confirm itself with a toast of its own. Returns false when there was nothing to undo.
   */
  undo: (id: string) => Promise<boolean>;
  /** Undoes the most recent action that can still be undone (the Ctrl Z shortcut). */
  undoLatest: () => Promise<boolean>;
}

let counter = 0;

export const useToastStore = create<ToastState>((set, get) => ({
  toasts: [],
  history: [],
  push: (input) => {
    counter += 1;
    const id = `t${counter}`;
    // The same message again (a command run five times that fails the same way) takes the place
    // of the one showing and counts up ("x3"), with a fresh timer. A toast with Undo is an action
    // of its own and always gets its own place, so each of them stays undoable.
    const repeats = (toast: ToastEntry) =>
      input.undo === undefined &&
      toast.undo === undefined &&
      toast.message === input.message &&
      toast.action?.label === input.action?.label &&
      (toast.tone ?? "default") === (input.tone ?? "default");
    const earlier = get().toasts.find(repeats);
    const entry: ToastEntry = { ...input, id, count: (earlier?.count ?? 0) + 1 };
    set((state) => ({
      toasts: [...state.toasts.filter((toast) => !repeats(toast)), entry].slice(-MAX_TOASTS),
      history:
        input.undo === undefined ? state.history : [...state.history, entry].slice(-UNDO_HISTORY),
    }));
    return id;
  },
  dismiss: (id) => set((state) => ({ toasts: state.toasts.filter((toast) => toast.id !== id) })),
  undo: async (id) => {
    const toast = get().history.find((entry) => entry.id === id);
    if (toast?.undo === undefined) return false;
    // Removing first makes a double click or a repeated shortcut run the undo only once.
    set((state) => ({
      toasts: state.toasts.filter((entry) => entry.id !== id),
      history: state.history.filter((entry) => entry.id !== id),
    }));
    const before = counter;
    try {
      await toast.undo();
    } catch (error) {
      notify({
        severity: "error",
        title: t("toast.couldNotUndo", { message: toast.message }),
        detail: error instanceof Error ? error.message : String(error),
      });
      return false;
    }
    // An undo that said what it did needs no second word.
    if (counter === before) get().push({ message: t("toast.undone", { message: toast.message }) });
    return true;
  },
  undoLatest: async () => {
    const latest = get().history.at(-1);
    return latest === undefined ? false : get().undo(latest.id);
  },
}));

/** Shows a toast. Pass `undo` for a reversible action; use the notification centre for errors. */
export function showToast(input: ToastInput): string {
  return useToastStore.getState().push(input);
}
